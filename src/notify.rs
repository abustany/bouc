use std::sync::Arc;

use anyhow::{Context, Result, bail};
use jiff::civil::Date;
use rand::Rng;

use crate::{
    bookings::{
        Booking, BookingLogEntry, BookingLogEntryPayload, ListBookingsFilter,
        NotificationSubscription, NotificationSubscriptionPayload, Person, PersonId, Repository,
    },
    email::{Message, Sender},
    strings::Locale,
};

struct StayNotificationInfo {
    recipient_name: String,
    recipient_email: String,
    booker_name: String,
    guest_count: u32,
    start_date: Date,
    end_date: Date,
    unsubscribe_token: String,
}

pub struct Notifier {
    repository: Arc<dyn Repository>,
    email_sender: Box<dyn Sender>,
    from_address: String,
    default_locale: Locale,
    base_url: String,
}

impl Notifier {
    pub fn new(
        repository: Arc<dyn Repository>,
        email_sender: Box<dyn Sender>,
        from_address: &str,
        default_locale: Locale,
        base_url: &str,
    ) -> Self {
        Self {
            repository,
            email_sender,
            from_address: from_address.to_owned(),
            default_locale,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    pub async fn send_verification_email(
        &self,
        person_id: PersonId,
        email: &str,
        locale: Locale,
        verification_token: &str,
    ) -> Result<()> {
        let person = self
            .repository
            .get_person(person_id)
            .await
            .context("getting person")?
            .context("user does not exist")?;
        let body = format!(
            "{}\n\n{}\n\n{}",
            locale.strings().notification_greeting(&person.name),
            locale
                .strings()
                .verification_email_body(&self.verification_link(person_id, verification_token)),
            locale.strings().notification_signature,
        );
        self.email_sender
            .send(Message {
                from: self.from_address.clone(),
                to: email.to_owned(),
                subject: locale.strings().verification_email_subject.to_owned(),
                body,
            })
            .await
            .context("error sending email")
    }

    fn verification_link(&self, person_id: PersonId, verification_token: &str) -> String {
        format!(
            "{}/notifications/verify/{}/{}",
            self.base_url,
            u32::from(person_id),
            verification_token
        )
    }

    fn unsubscribe_link(&self, person_id: PersonId, unsubscribe_token: &str) -> String {
        format!(
            "{}/notifications/unsubscribe/{}/{}",
            self.base_url,
            u32::from(person_id),
            unsubscribe_token
        )
    }

    async fn notify_intersecting_bookings<Fut: Future<Output = Result<()>>>(
        &self,
        booking: &Booking,
        notify: impl Fn(PersonId, Locale, StayNotificationInfo) -> Fut,
    ) -> Result<()> {
        let intersecting_bookings = self
            .repository
            .list_bookings(ListBookingsFilter::IntersectsRange {
                start: booking.start_date,
                end: booking.end_date,
            })
            .await
            .context("error listing intersecting bookings")?;

        if intersecting_bookings.is_empty() {
            return Ok(());
        }

        let Person {
            name: booker_name, ..
        } = self
            .repository
            .get_person(booking.creator_id)
            .await
            .context("error getting booking creator")?
            .context("booking creator does not exist")?;

        let mut errors: Vec<anyhow::Error> = Vec::new();

        for b in intersecting_bookings {
            if b.creator_id == booking.creator_id {
                continue;
            }

            let Some(NotificationSubscription {
                                payload: NotificationSubscriptionPayload::Active { locale: locale_id, email, unsubscribe_token },
                                ..
                            }) = self.repository
                                .get_notification_subscription(b.creator_id)
                                .await
                                .with_context(|| {
                                    format!(
                                        "error getting creator notification subscription for intersecting booking {:?}",
                                        b.id
                                    )
                                })?
                            else {
                                continue;
                            };

            let Person { name, .. } = self
                .repository
                .get_person(b.creator_id)
                .await
                .with_context(|| {
                    format!("error getting creator for intersecting booking {:?}", b.id)
                })?
                .with_context(|| {
                    format!("creator does not exist for intersecting booking {:?}", b.id)
                })?;

            if let Err(err) = notify(
                b.creator_id,
                Locale::from_language_tag(&locale_id).unwrap_or(self.default_locale),
                StayNotificationInfo {
                    recipient_name: name,
                    recipient_email: email.clone(),
                    booker_name: booker_name.clone(),
                    guest_count: booking.guest_count,
                    start_date: booking.start_date.max(b.start_date),
                    end_date: booking.end_date.min(b.end_date),
                    unsubscribe_token,
                },
            )
            .await
            .with_context(|| {
                format!(
                    "error sending notification for intersecting booking {:?} to {email}",
                    b.id
                )
            }) {
                errors.push(err);
            }
        }

        if !errors.is_empty() {
            bail!(
                "error sending {} notification(s): {}",
                errors.len(),
                errors
                    .iter()
                    .map(|err| format!("{err:#}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }

        Ok(())
    }

    pub async fn notify_booking_changed(&self, entry: &BookingLogEntry) -> Result<()> {
        match &entry.payload {
            BookingLogEntryPayload::BookingCreated { booking } => {
                self.notify_intersecting_bookings(booking, |person_id, locale, info| async move {
                    self.notify_will_stay_with_you(person_id, locale, &info)
                        .await
                })
                .await
            }
            BookingLogEntryPayload::BookingChanged { before, after } => {
                self.notify_intersecting_bookings(before, |person_id, locale, info| async move {
                    self.notify_will_not_stay_with_you_anymore(person_id, locale, &info)
                        .await
                })
                .await?;
                self.notify_intersecting_bookings(after, |person_id, locale, info| async move {
                    self.notify_will_stay_with_you(person_id, locale, &info)
                        .await
                })
                .await
            }
            BookingLogEntryPayload::BookingDeleted { booking } => {
                self.notify_intersecting_bookings(booking, |person_id, locale, info| async move {
                    self.notify_will_not_stay_with_you_anymore(person_id, locale, &info)
                        .await
                })
                .await
            }
        }
    }

    async fn notify_will_stay_with_you(
        &self,
        person_id: PersonId,
        locale: Locale,
        info: &StayNotificationInfo,
    ) -> Result<()> {
        let s = locale.strings();
        let body = format!(
            "{}\n\n{}\n\n{}n\n\n{}\n{}",
            s.notification_greeting(&info.recipient_name),
            s.notification_will_stay_with_you_body(
                &info.booker_name,
                info.guest_count,
                &info.start_date,
                &info.end_date
            ),
            s.notification_signature,
            s.notification_unsubscribe,
            self.unsubscribe_link(person_id, &info.unsubscribe_token),
        );
        self.email_sender
            .send(Message {
                from: self.from_address.clone(),
                to: info.recipient_email.clone(),
                subject: locale
                    .strings()
                    .notification_will_stay_with_you_subject
                    .to_owned(),
                body,
            })
            .await
            .context("error sending email")
    }

    async fn notify_will_not_stay_with_you_anymore(
        &self,
        person_id: PersonId,
        locale: Locale,
        info: &StayNotificationInfo,
    ) -> Result<()> {
        let s = locale.strings();
        let body = format!(
            "{}\n\n{}\n\n{}\n\n{}\n{}",
            s.notification_greeting(&info.recipient_name),
            s.notification_will_not_stay_with_you_anymore_body(
                &info.booker_name,
                info.guest_count,
                &info.start_date,
                &info.end_date
            ),
            s.notification_signature,
            s.notification_unsubscribe,
            self.unsubscribe_link(person_id, &info.unsubscribe_token)
        );
        self.email_sender
            .send(Message {
                from: self.from_address.clone(),
                to: info.recipient_email.clone(),
                subject: locale
                    .strings()
                    .notification_will_not_stay_with_you_anymore_subject
                    .to_owned(),
                body,
            })
            .await
            .context("error sending email")
    }
}

pub fn generate_verification_token() -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}
