use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use jiff::Timestamp;
use tokio_rusqlite::rusqlite::OptionalExtension;
use tokio_rusqlite::{Connection, rusqlite};

use crate::bookings::{
    Booking, BookingId, BookingInput, BookingLogEntry, BookingLogEntryId, BookingLogEntryPayload,
    DeleteBookingError, ListBookingsFilter, NotificationSubscription, Person, PersonId, Repository,
    UpdateBookingError,
};

const BOOKING_LOG_PAGE_SIZE: u32 = 50;

/// SQLite-backed [`Repository`].
pub struct SqliteRepository {
    conn: Connection,
}

pub const MEMORY_DB: &str = ":memory:";

impl SqliteRepository {
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(path)
            .await
            .context("opening sqlite database")?;
        conn.call(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            )?;
            Ok(())
        })
        .await
        .context("configuring sqlite connection")?;
        conn.call(|conn| -> rusqlite::Result<()> {
            let initialized: bool = conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'people'",
                [],
                |row| row.get::<_, i64>(0),
            )? > 0;
            if !initialized {
                conn.execute_batch(include_str!("../schema.sql"))?;
            }
            Ok(())
        })
        .await
        .context("applying schema")?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl Repository for SqliteRepository {
    async fn save_person(&self, name: &str) -> anyhow::Result<Person> {
        let name = name.to_owned();
        self.conn
            .call(move |conn| -> rusqlite::Result<Person> {
                let id: i64 = conn.query_row(
                    "INSERT INTO people (name) VALUES (?1) \
                     ON CONFLICT(name) DO UPDATE SET name = excluded.name \
                     RETURNING id",
                    rusqlite::params![name],
                    |row| row.get(0),
                )?;

                Ok(Person {
                    id: PersonId::new(parse_u32(id)?),
                    name,
                })
            })
            .await
            .context("saving person")
    }

    async fn list_people(&self) -> anyhow::Result<Vec<Person>> {
        self.conn
            .call(|conn| -> rusqlite::Result<Vec<Person>> {
                let mut stmt = conn.prepare("SELECT id, name FROM people ORDER BY name")?;
                let people = stmt
                    .query_map([], |row| {
                        Ok(Person {
                            id: PersonId::new(parse_u32(row.get(0)?)?),
                            name: row.get(1)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(people)
            })
            .await
            .context("listing people")
    }

    async fn get_notification_subscription(
        &self,
        person_id: PersonId,
    ) -> anyhow::Result<Option<NotificationSubscription>> {
        self.conn
            .call(
                move |conn| -> rusqlite::Result<Option<NotificationSubscription>> {
                    conn.query_row(
                        "SELECT payload FROM notification_subscriptions WHERE person_id = ?1",
                        rusqlite::params![u32::from(person_id)],
                        |row| {
                            Ok(NotificationSubscription {
                                person_id,
                                payload: parse_json(&row.get::<_, String>(0)?)?,
                            })
                        },
                    )
                    .optional()
                },
            )
            .await
            .context("getting notification subscription")
    }

    async fn save_notification_subscription(
        &self,
        subscription: &NotificationSubscription,
    ) -> anyhow::Result<()> {
        let subscription = subscription.clone();
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO notification_subscriptions (person_id, payload) \
                     VALUES (?1, ?2) \
                     ON CONFLICT(person_id) DO UPDATE SET payload = excluded.payload",
                    rusqlite::params![
                        u32::from(subscription.person_id),
                        to_json(&subscription.payload)?,
                    ],
                )?;
                Ok(())
            })
            .await
            .context("saving notification subscription")
    }

    async fn create_booking(
        &self,
        booking: &BookingInput,
    ) -> anyhow::Result<(Booking, BookingLogEntry)> {
        let booking = booking.clone();
        self.conn
            .call(
                move |conn| -> rusqlite::Result<(Booking, BookingLogEntry)> {
                    let tx = conn.transaction()?;

                    let new_id: i64 = tx.query_row(
                        "INSERT INTO bookings (start_date, end_date, creator_id, guest_count) \
                         VALUES (?1, ?2, ?3, ?4) RETURNING id",
                        rusqlite::params![
                            format_date(&booking.start_date()),
                            format_date(&booking.end_date()),
                            u32::from(booking.creator_id()),
                            booking.guest_count(),
                        ],
                        |row| row.get(0),
                    )?;

                    let created = persisted(BookingId::new(parse_u32(new_id)?), booking);
                    let log_entry = append_log(
                        &tx,
                        created.creator_id,
                        BookingLogEntryPayload::BookingCreated {
                            booking: created.clone(),
                        },
                    )?;

                    tx.commit()?;
                    Ok((created, log_entry))
                },
            )
            .await
            .context("creating booking")
    }

    async fn update_booking(
        &self,
        id: BookingId,
        creator_id: PersonId,
        booking: &BookingInput,
    ) -> Result<(Booking, BookingLogEntry), UpdateBookingError> {
        let booking = booking.clone();
        self.conn
            .call(
                move |conn| -> rusqlite::Result<Option<(Booking, BookingLogEntry)>> {
                    let tx = conn.transaction()?;

                    let before = tx
                        .query_row(
                            "SELECT start_date, end_date, guest_count FROM bookings \
                             WHERE id = ?1 AND creator_id = ?2",
                            rusqlite::params![u32::from(id), u32::from(creator_id)],
                            |row| {
                                Ok(Booking {
                                    id,
                                    start_date: parse_date(&row.get::<_, String>(0)?)?,
                                    end_date: parse_date(&row.get::<_, String>(1)?)?,
                                    creator_id,
                                    guest_count: parse_u32(row.get(2)?)?,
                                })
                            },
                        )
                        .optional()?;

                    let Some(before) = before else {
                        return Ok(None);
                    };

                    tx.execute(
                        "UPDATE bookings \
                         SET start_date = ?1, end_date = ?2, guest_count = ?3 \
                         WHERE id = ?4",
                        rusqlite::params![
                            format_date(&booking.start_date()),
                            format_date(&booking.end_date()),
                            booking.guest_count(),
                            u32::from(id),
                        ],
                    )?;

                    let after = Booking {
                        id,
                        start_date: booking.start_date(),
                        end_date: booking.end_date(),
                        creator_id,
                        guest_count: booking.guest_count(),
                    };
                    let log_entry = append_log(
                        &tx,
                        creator_id,
                        BookingLogEntryPayload::BookingChanged {
                            before,
                            after: after.clone(),
                        },
                    )?;

                    tx.commit()?;
                    Ok(Some((after, log_entry)))
                },
            )
            .await
            .map_err(|e| UpdateBookingError::Internal(e.into()))?
            .ok_or(UpdateBookingError::NotFound)
    }

    async fn list_bookings(&self, filter: ListBookingsFilter) -> anyhow::Result<Vec<Booking>> {
        let (condition, params) = match filter {
            ListBookingsFilter::EndsAfter(date) => ("end_date >= ?1", vec![format_date(&date)]),
            ListBookingsFilter::IntersectsRange { start, end } => (
                "start_date <= ?2 AND end_date >= ?1",
                vec![format_date(&start), format_date(&end)],
            ),
        };

        self.conn
            .call(move |conn| -> rusqlite::Result<Vec<Booking>> {
                let mut stmt = conn.prepare(&format!(
                    "SELECT id, start_date, end_date, creator_id, guest_count \
                     FROM bookings WHERE {condition} ORDER BY start_date, id"
                ))?;
                let bookings = stmt
                    .query_map(rusqlite::params_from_iter(params), |row| {
                        Ok(Booking {
                            id: BookingId::new(parse_u32(row.get(0)?)?),
                            start_date: parse_date(&row.get::<_, String>(1)?)?,
                            end_date: parse_date(&row.get::<_, String>(2)?)?,
                            creator_id: PersonId::new(parse_u32(row.get(3)?)?),
                            guest_count: parse_u32(row.get(4)?)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(bookings)
            })
            .await
            .context("listing bookings")
    }

    async fn delete_booking(
        &self,
        id: BookingId,
        creator_id: PersonId,
    ) -> Result<BookingLogEntry, DeleteBookingError> {
        self.conn
            .call(move |conn| -> rusqlite::Result<Option<BookingLogEntry>> {
                let tx = conn.transaction()?;

                let deleted = tx
                    .query_row(
                        "DELETE FROM bookings WHERE id = ?1 AND creator_id = ?2 \
                         RETURNING start_date, end_date, guest_count",
                        rusqlite::params![u32::from(id), u32::from(creator_id)],
                        |row| {
                            Ok(Booking {
                                id,
                                start_date: parse_date(&row.get::<_, String>(0)?)?,
                                end_date: parse_date(&row.get::<_, String>(1)?)?,
                                creator_id,
                                guest_count: parse_u32(row.get(2)?)?,
                            })
                        },
                    )
                    .optional()?;

                let Some(booking) = deleted else {
                    return Ok(None);
                };

                let log_entry = append_log(
                    &tx,
                    creator_id,
                    BookingLogEntryPayload::BookingDeleted { booking },
                )?;

                tx.commit()?;
                Ok(Some(log_entry))
            })
            .await
            .map_err(|e| DeleteBookingError::Internal(e.into()))?
            .ok_or(DeleteBookingError::NotFound)
    }

    async fn list_booking_log(
        &self,
        before: Option<BookingLogEntryId>,
    ) -> anyhow::Result<Vec<BookingLogEntry>> {
        let before = before.map(u32::from);
        self.conn
            .call(move |conn| -> rusqlite::Result<Vec<BookingLogEntry>> {
                let mut stmt = conn.prepare(
                    "SELECT id, creator_id, create_time, payload FROM bookings_log \
                     WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
                )?;
                let entries = stmt
                    .query_map(rusqlite::params![before, BOOKING_LOG_PAGE_SIZE], |row| {
                        Ok(BookingLogEntry {
                            id: BookingLogEntryId::new(parse_u32(row.get(0)?)?),
                            creator_id: PersonId::new(parse_u32(row.get(1)?)?),
                            create_time: parse_timestamp(&row.get::<_, String>(2)?)?,
                            payload: parse_json(&row.get::<_, String>(3)?)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(entries)
            })
            .await
            .context("listing booking log")
    }
}

fn append_log(
    tx: &rusqlite::Transaction,
    creator_id: PersonId,
    payload: BookingLogEntryPayload,
) -> rusqlite::Result<BookingLogEntry> {
    let create_time = Timestamp::now();

    let id: i64 = tx.query_row(
        "INSERT INTO bookings_log (creator_id, create_time, payload) \
         VALUES (?1, ?2, ?3) RETURNING id",
        rusqlite::params![
            u32::from(creator_id),
            create_time.to_string(),
            to_json(&payload)?,
        ],
        |row| row.get(0),
    )?;

    Ok(BookingLogEntry {
        id: BookingLogEntryId::new(parse_u32(id)?),
        creator_id,
        create_time,
        payload,
    })
}

fn format_date(d: &jiff::civil::Date) -> String {
    d.strftime("%Y%m%d").to_string()
}

fn parse_date(s: &str) -> rusqlite::Result<jiff::civil::Date> {
    jiff::civil::Date::strptime("%Y%m%d", s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

fn parse_timestamp(s: &str) -> rusqlite::Result<Timestamp> {
    s.parse().map_err(|e: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

fn parse_json<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

fn to_json<T: serde::Serialize>(payload: &T) -> rusqlite::Result<String> {
    serde_json::to_string(payload).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
}

fn parse_u32(value: i64) -> rusqlite::Result<u32> {
    value.try_into().map_err(|e: std::num::TryFromIntError| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, e.into())
    })
}

fn persisted(id: BookingId, booking: BookingInput) -> Booking {
    Booking {
        id,
        start_date: booking.start_date(),
        end_date: booking.end_date(),
        creator_id: booking.creator_id(),
        guest_count: booking.guest_count(),
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::bookings::NotificationSubscriptionPayload;

    async fn setup() -> SqliteRepository {
        let conn = Connection::open_in_memory().await.unwrap();
        conn.call(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(include_str!("../schema.sql"))?;
            Ok(())
        })
        .await
        .unwrap();
        SqliteRepository { conn }
    }

    fn start_date() -> jiff::civil::Date {
        date(2026, 7, 24)
    }

    fn end_date() -> jiff::civil::Date {
        date(2026, 7, 26)
    }

    async fn booking_for(repo: &SqliteRepository, creator: PersonId, guests: u32) -> Booking {
        repo.create_booking(&BookingInput::new(start_date(), end_date(), creator, guests).unwrap())
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn creates_then_updates_booking() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        assert_eq!(person.name, "Alice");

        let created = booking_for(&repo, person.id, 3).await;
        assert_eq!(created.guest_count, 3);

        let (updated, log_entry) = repo
            .update_booking(
                created.id,
                person.id,
                &BookingInput::new(start_date(), end_date(), person.id, 5).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.guest_count, 5);
        assert!(matches!(
            log_entry.payload,
            BookingLogEntryPayload::BookingChanged { before, after }
                if before.guest_count == 3 && after.guest_count == 5
        ));
    }

    #[tokio::test]
    async fn saving_existing_name_returns_existing_row() {
        let repo = setup().await;

        let first = repo.save_person("Alice").await.unwrap();
        let again = repo.save_person("Alice").await.unwrap();

        assert_eq!(first.id, again.id);
        assert_eq!(again.name, "Alice");
    }

    #[tokio::test]
    async fn notification_subscription_is_saved_then_updated() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        let other_person = repo.save_person("Bob").await.unwrap();

        assert!(
            repo.get_notification_subscription(person.id)
                .await
                .unwrap()
                .is_none()
        );

        repo.save_notification_subscription(&NotificationSubscription {
            person_id: person.id,
            payload: NotificationSubscriptionPayload::Pending {
                email: "alice@example.com".to_owned(),
                verification_token: "verification_token".to_owned(),
            },
        })
        .await
        .unwrap();

        let saved = repo
            .get_notification_subscription(person.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.person_id, person.id);
        assert!(matches!(
            saved.payload,
            NotificationSubscriptionPayload::Pending { email, verification_token } if email == "alice@example.com" && verification_token == "verification_token"
        ));

        repo.save_notification_subscription(&NotificationSubscription {
            person_id: person.id,
            payload: NotificationSubscriptionPayload::Active {
                email: "alice@example.com".to_owned(),
                unsubscribe_token: "unsubscribe_token".to_owned(),
            },
        })
        .await
        .unwrap();

        let updated = repo
            .get_notification_subscription(person.id)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            updated.payload,
            NotificationSubscriptionPayload::Active { email, unsubscribe_token, locale } if email == "alice@example.com" && unsubscribe_token == "unsubscribe_token" && locale == "en"
        ));

        assert!(
            repo.get_notification_subscription(other_person.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn list_bookings_filters_and_orders() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();

        let (past, _) = repo
            .create_booking(
                &BookingInput::new(date(2026, 1, 10), date(2026, 1, 12), person.id, 3).unwrap(),
            )
            .await
            .unwrap();
        let (later, _) = repo
            .create_booking(
                &BookingInput::new(date(2026, 8, 5), date(2026, 8, 7), person.id, 3).unwrap(),
            )
            .await
            .unwrap();
        let (ongoing, _) = repo
            .create_booking(
                &BookingInput::new(date(2026, 7, 20), date(2026, 7, 28), person.id, 3).unwrap(),
            )
            .await
            .unwrap();

        let bookings = repo
            .list_bookings(ListBookingsFilter::EndsAfter(date(2026, 7, 27)))
            .await
            .unwrap();
        let ids: Vec<_> = bookings.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![ongoing.id, later.id]);
        assert!(!ids.contains(&past.id));
        assert_eq!(bookings[0].start_date, date(2026, 7, 20));
        assert_eq!(bookings[0].end_date, date(2026, 7, 28));

        let bookings = repo
            .list_bookings(ListBookingsFilter::IntersectsRange {
                start: date(2026, 7, 25),
                end: date(2026, 7, 27),
            })
            .await
            .unwrap();
        let ids: Vec<_> = bookings.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![ongoing.id]);

        let bookings = repo
            .list_bookings(ListBookingsFilter::IntersectsRange {
                start: date(2026, 7, 28),
                end: date(2026, 8, 5),
            })
            .await
            .unwrap();
        let ids: Vec<_> = bookings.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![ongoing.id, later.id]);

        let bookings = repo
            .list_bookings(ListBookingsFilter::IntersectsRange {
                start: date(2026, 7, 29),
                end: date(2026, 8, 4),
            })
            .await
            .unwrap();
        assert!(bookings.is_empty());
    }

    #[tokio::test]
    async fn deletes_booking_and_fails_on_missing_ones() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        let other_person = repo.save_person("Bob").await.unwrap();
        let created = booking_for(&repo, person.id, 3).await;

        assert!(matches!(
            repo.delete_booking(created.id, other_person.id).await,
            Err(DeleteBookingError::NotFound)
        ));
        assert!(
            repo.list_bookings(ListBookingsFilter::EndsAfter(start_date()))
                .await
                .unwrap()
                .len()
                == 1
        );

        let log_entry = repo.delete_booking(created.id, person.id).await.unwrap();
        assert!(matches!(
            log_entry.payload,
            BookingLogEntryPayload::BookingDeleted { booking } if booking.id == created.id
        ));
        assert!(
            repo.list_bookings(ListBookingsFilter::EndsAfter(start_date()))
                .await
                .unwrap()
                .is_empty()
        );

        assert!(matches!(
            repo.delete_booking(created.id, person.id).await,
            Err(DeleteBookingError::NotFound)
        ));
    }

    #[tokio::test]
    async fn updating_missing_booking_is_not_found() {
        let repo = setup().await;
        let person = repo.save_person("Bob").await.unwrap();
        let booking = BookingInput::new(start_date(), end_date(), person.id, 3).unwrap();
        assert!(matches!(
            repo.update_booking(BookingId::new(999), person.id, &booking)
                .await,
            Err(UpdateBookingError::NotFound)
        ));
        assert!(repo.list_booking_log(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn updating_somebody_elses_booking_is_not_found() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        let other_person = repo.save_person("Bob").await.unwrap();
        let created = booking_for(&repo, person.id, 3).await;

        let booking = BookingInput::new(start_date(), end_date(), other_person.id, 5).unwrap();
        assert!(matches!(
            repo.update_booking(created.id, other_person.id, &booking)
                .await,
            Err(UpdateBookingError::NotFound)
        ));

        let bookings = repo
            .list_bookings(ListBookingsFilter::EndsAfter(start_date()))
            .await
            .unwrap();
        assert_eq!(bookings[0].guest_count, 3);
        assert_eq!(bookings[0].creator_id, person.id);
        assert_eq!(repo.list_booking_log(None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn logs_creations_changes_and_deletions() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();

        let (created, create_entry) = repo
            .create_booking(&BookingInput::new(start_date(), end_date(), person.id, 3).unwrap())
            .await
            .unwrap();
        let (_, update_entry) = repo
            .update_booking(
                created.id,
                person.id,
                &BookingInput::new(start_date(), end_date(), person.id, 5).unwrap(),
            )
            .await
            .unwrap();
        let delete_entry = repo.delete_booking(created.id, person.id).await.unwrap();

        let entries = repo.list_booking_log(None).await.unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| e.creator_id == person.id));
        assert_eq!(
            entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![delete_entry.id, update_entry.id, create_entry.id]
        );

        assert!(matches!(
            &entries[2].payload,
            BookingLogEntryPayload::BookingCreated { booking } if booking.guest_count == 3
        ));
        assert!(matches!(
            &entries[1].payload,
            BookingLogEntryPayload::BookingChanged { before, after }
                if before.guest_count == 3 && after.guest_count == 5
        ));
        assert!(matches!(
            &entries[0].payload,
            BookingLogEntryPayload::BookingDeleted { booking } if booking.guest_count == 5
        ));
    }

    #[tokio::test]
    async fn booking_log_is_paginated_newest_first() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();

        let total = BOOKING_LOG_PAGE_SIZE + 10;
        for guests in 1..=total {
            booking_for(&repo, person.id, guests).await;
        }

        let first = repo.list_booking_log(None).await.unwrap();
        assert_eq!(first.len(), usize::try_from(BOOKING_LOG_PAGE_SIZE).unwrap());

        let second = repo
            .list_booking_log(Some(first.last().unwrap().id))
            .await
            .unwrap();
        assert_eq!(second.len(), 10);

        let ids: Vec<u32> = first
            .iter()
            .chain(second.iter())
            .map(|e| u32::from(e.id))
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();
        assert_eq!(ids, sorted);
        assert_eq!(ids.len(), usize::try_from(total).unwrap());
    }
}
