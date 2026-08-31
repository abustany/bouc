use std::collections::HashMap;
use std::sync::LazyLock;

use icu::calendar::{Date as IcuDate, Iso};
use icu::datetime::DateTimeFormatter;
use icu::datetime::fieldsets::{M, MD, YMD};
use icu::locale::locale;
use jiff_icu::ConvertInto;
use serde::Serialize;

use crate::interpolate::interpolate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    Fr,
    #[default]
    En,
}

impl Locale {
    pub fn strings(self) -> &'static Strings {
        match self {
            Locale::Fr => &FR,
            Locale::En => &EN,
        }
    }

    pub fn from_accept_language(header: &str) -> Self {
        let mut best: Option<(Locale, f32)> = None;

        for entry in header.split(',') {
            let mut parts = entry.split(';');
            let tag = parts.next().unwrap_or_default().trim();
            let quality = parts
                .find_map(|p| p.trim().strip_prefix("q=")?.parse::<f32>().ok())
                .unwrap_or(1.0);

            let Some(locale) = Locale::from_language_tag(tag) else {
                continue;
            };

            if best.is_none_or(|(_, best_quality)| quality > best_quality) {
                best = Some((locale, quality));
            }
        }

        best.map(|(locale, _)| locale).unwrap_or_default()
    }

    pub fn from_language_tag(tag: &str) -> Option<Self> {
        let primary = tag.split('-').next()?;

        if primary.eq_ignore_ascii_case("fr") {
            Some(Locale::Fr)
        } else if primary.eq_ignore_ascii_case("en") {
            Some(Locale::En)
        } else {
            None
        }
    }
}

#[derive(Serialize)]
pub struct Strings {
    pub lang: &'static str,
    pub index_hint: &'static str,
    pub index_hint_touch: &'static str,
    pub index_hint_end_day: &'static str,
    pub index_hint_end_day_touch: &'static str,
    pub index_hint_booking_complete_info: &'static str,
    pub index_hint_booking_notifications_cta: &'static str,
    pub index_hint_booking_notifications_pending: &'static str,
    pub index_hint_booking_notifications_active: &'static str,
    pub modal_close: &'static str,
    pub day_popover_empty: &'static str,
    pub day_popover_edit_button_title: &'static str,
    pub day_popover_delete_button_title: &'static str,
    pub day_at_capacity: &'static str,
    pub day_popover_confirm_delete_message: &'static str,
    pub guest_one: &'static str,
    pub guest_many: &'static str,
    pub form_name_placeholder: &'static str,
    pub form_add: &'static str,
    pub form_saving: &'static str,
    pub internal_error: &'static str,
    pub name_empty: &'static str,
    pub booking_end_before_start: &'static str,
    pub booking_guest_count_zero: &'static str,
    pub unknown_person: &'static str,
    pub start_booking: &'static str,
    pub booking_modal_title_new: &'static str,
    pub booking_modal_title_edit: &'static str,
    pub booking_modal_dates: &'static str,
    pub booking_modal_guest_count: &'static str,
    pub booking_modal_save: &'static str,
    pub name_modal_title: &'static str,
    pub name_modal_name: &'static str,
    pub name_modal_save: &'static str,
    pub email_modal_title: &'static str,
    pub email_modal_name: &'static str,
    pub email_modal_save: &'static str,
    pub profile_login: &'static str,
    pub profile_notifications_status: &'static str,
    pub profile_notifications_status_none: &'static str,
    pub profile_notifications_status_pending: &'static str,
    pub profile_notifications_status_active: &'static str,
    pub profile_notifications_status_disabled: &'static str,
    pub profile_notifications_enable: &'static str,
    pub profile_notifications_resend_verification: &'static str,
    pub profile_notifications_disable: &'static str,
    pub profile_disconnect: &'static str,
    pub log_booking_created_title_suffix: &'static str,
    pub log_booking_changed_title_suffix: &'static str,
    pub log_booking_deleted_title_suffix: &'static str,
    pub navbar_link_calendar: &'static str,
    pub navbar_link_log: &'static str,
    pub notification_greeting: &'static str,
    pub notification_will_stay_with_you_subject: &'static str,
    pub notification_will_stay_with_you_body: &'static str,
    pub notification_will_not_stay_with_you_anymore_subject: &'static str,
    pub notification_will_not_stay_with_you_anymore_body: &'static str,
    pub notification_signature: &'static str,
    pub notification_unsubscribe: &'static str,
    pub verification_email_subject: &'static str,
    pub verification_email_body: &'static str,
    pub email_verified_ok: &'static str,
    pub email_verified_error: &'static str,
    pub email_unsubscribed_ok: &'static str,
    pub email_unsubscribed_error: &'static str,
    pub email_you_can_close: &'static str,

    #[serde(skip)]
    month_formatter: &'static LazyLock<DateTimeFormatter<M>>,
    #[serde(skip)]
    month_day_formatter: &'static LazyLock<DateTimeFormatter<MD>>,
    #[serde(skip)]
    year_month_day_formatter: &'static LazyLock<DateTimeFormatter<YMD>>,
}

impl Strings {
    pub fn guests(&self, count: u32) -> String {
        let unit = if count > 1 {
            self.guest_many
        } else {
            self.guest_one
        };
        format!("{count} {unit}")
    }

    pub fn log_booking_created_title(&self, actor_name: &str) -> String {
        format!("{actor_name} {}", self.log_booking_created_title_suffix)
    }

    pub fn log_booking_changed_title(&self, actor_name: &str) -> String {
        format!("{actor_name} {}", self.log_booking_changed_title_suffix)
    }

    pub fn log_booking_deleted_title(&self, actor_name: &str) -> String {
        format!("{actor_name} {}", self.log_booking_deleted_title_suffix)
    }

    pub fn notification_greeting(&self, recipient_name: &str) -> String {
        interpolate(
            self.notification_greeting,
            &HashMap::from([("recipient_name", recipient_name)]),
        )
        .expect("error formatting")
    }

    pub fn notification_will_stay_with_you_body(
        &self,
        booker_name: &str,
        guest_count: u32,
        start_date: &jiff::civil::Date,
        end_date: &jiff::civil::Date,
    ) -> String {
        interpolate(
            self.notification_will_stay_with_you_body,
            &HashMap::from([
                ("booker_name", booker_name),
                ("guest_count", &guest_count.to_string()),
                ("start_date", &self.format_date_year_month_day(*start_date)),
                ("end_date", &self.format_date_year_month_day(*end_date)),
            ]),
        )
        .expect("error formatting")
    }

    pub fn notification_will_not_stay_with_you_anymore_body(
        &self,
        booker_name: &str,
        guest_count: u32,
        start_date: &jiff::civil::Date,
        end_date: &jiff::civil::Date,
    ) -> String {
        interpolate(
            self.notification_will_not_stay_with_you_anymore_body,
            &HashMap::from([
                ("booker_name", booker_name),
                ("guest_count", &guest_count.to_string()),
                ("start_date", &self.format_date_year_month_day(*start_date)),
                ("end_date", &self.format_date_year_month_day(*end_date)),
            ]),
        )
        .expect("error formatting")
    }

    pub fn verification_email_body(&self, verification_link: &str) -> String {
        interpolate(
            self.verification_email_body,
            &HashMap::from([("verification_link", verification_link)]),
        )
        .expect("error formatting")
    }

    pub fn format_date_month_name(&self, d: impl ConvertInto<IcuDate<Iso>>) -> String {
        ucfirst(&self.month_formatter.format(&d.convert_into()).to_string())
    }

    pub fn format_date_month_day(&self, d: impl ConvertInto<IcuDate<Iso>>) -> String {
        self.month_day_formatter
            .format(&d.convert_into())
            .to_string()
    }

    pub fn format_date_year_month_day(&self, d: impl ConvertInto<IcuDate<Iso>>) -> String {
        self.year_month_day_formatter
            .format(&d.convert_into())
            .to_string()
    }
}

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

static MONTH_FORMATTER_FR: LazyLock<DateTimeFormatter<M>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("fr").into(), M::long())
        .expect("failed to build French month formatter")
});

static MONTH_FORMATTER_EN: LazyLock<DateTimeFormatter<M>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("en").into(), M::long())
        .expect("failed to build English month formatter")
});

static MONTH_DAY_FORMATTER_FR: LazyLock<DateTimeFormatter<MD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("fr").into(), MD::long())
        .expect("failed to build French month day formatter")
});

static MONTH_DAY_FORMATTER_EN: LazyLock<DateTimeFormatter<MD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("en").into(), MD::long())
        .expect("failed to build English month day formatter")
});

static YEAR_MONTH_DAY_FORMATTER_FR: LazyLock<DateTimeFormatter<YMD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("fr").into(), YMD::long())
        .expect("failed to build French month day year formatter")
});

static YEAR_MONTH_DAY_FORMATTER_EN: LazyLock<DateTimeFormatter<YMD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("en").into(), YMD::long())
        .expect("failed to build English month day year formatter")
});

static FR: Strings = Strings {
    lang: "fr",
    index_hint: "Pointer sur un jour pour entrer une réservation ou afficher les détails",
    index_hint_touch: "Toucher un jour pour entrer une réservation ou afficher les détails",
    index_hint_end_day: "Cliquer sur le dernier jour du séjour",
    index_hint_end_day_touch: "Toucher le dernier jour du séjour",
    index_hint_booking_complete_info: "Réservation bien enregistrée!",
    index_hint_booking_notifications_cta: "M'avertir si quelqu'un réserve sur cette période",
    index_hint_booking_notifications_pending: "Vérifie ta boite mail, un lien de vérification t'y attend",
    index_hint_booking_notifications_active: "Tu seras averti si quelqu'un réserve sur cette période",
    modal_close: "Fermer",
    day_popover_empty: "Aucune réservation",
    day_popover_edit_button_title: "Modifier",
    day_popover_delete_button_title: "Supprimer",
    day_at_capacity: "Complet",
    day_popover_confirm_delete_message: "Êtes vous sûr de vouloir supprimer cette réservation ?",
    guest_one: "personne",
    guest_many: "personnes",
    form_name_placeholder: "Nom",
    form_add: "Ajouter",
    form_saving: "Enregistrement…",
    internal_error: "Erreur interne du serveur",
    name_empty: "Le nom ne peut pas être vide",
    booking_end_before_start: "La date de fin est antérieure à la date de début",
    booking_guest_count_zero: "Il faut au moins une personne",
    unknown_person: "Illustre inconnu",
    start_booking: "Réserver…",
    booking_modal_title_new: "Nouvelle réservation",
    booking_modal_title_edit: "Modifier la réservation",
    booking_modal_dates: "Dates",
    booking_modal_guest_count: "Personnes",
    booking_modal_save: "Enregistrer",
    name_modal_title: "Comment t'appelles-tu ?",
    name_modal_name: "Nom",
    name_modal_save: "Enregistrer",
    email_modal_title: "Ton addresse email",
    email_modal_name: "Email",
    email_modal_save: "Enregistrer",
    profile_login: "S'identifier",
    profile_notifications_status: "Notifications",
    profile_notifications_status_none: "désactivées",
    profile_notifications_status_pending: "en attente de vérification",
    profile_notifications_status_active: "activées",
    profile_notifications_status_disabled: "désactivées",
    profile_notifications_enable: "Activer",
    profile_notifications_resend_verification: "Renvoyer le message",
    profile_notifications_disable: "Désactiver",
    profile_disconnect: "Se déconnecter",
    log_booking_created_title_suffix: "a ajouté une réservation",
    log_booking_changed_title_suffix: "a modifié une réservation",
    log_booking_deleted_title_suffix: "a supprimé une réservation",
    navbar_link_calendar: "Réservations",
    navbar_link_log: "Journal",
    notification_greeting: "Salut {recipient_name}!",
    notification_will_stay_with_you_subject: "Nouvelle réservation sur vos dates",
    notification_will_stay_with_you_body: concat!(
        "{booker_name} vient d'enregistrer une réservation pour {guest_count} ",
        "personne(s) et partagera son séjour avec toi du {start_date} au {end_date}.",
    ),
    notification_will_not_stay_with_you_anymore_subject: "Réservation supprimée sur vos dates",
    notification_will_not_stay_with_you_anymore_body: "{booker_name} a supprimé sa réservation pour {guest_count} personne(s) du {start_date} au {end_date}.",
    notification_signature: "Amicalement, le système de réservation",
    notification_unsubscribe: "Clique sur le lien suivant si tu ne souhaites plus recevoir de notifications:",
    verification_email_subject: "Vérification de l'adresse email",
    verification_email_body: concat!(
        "Clique sur le lien suivant pour vérifier ton addresse email:\n\n{verification_link}\n\n",
        "Si tu n'as pas demandé à recevoir de notifications, tu peux ignorer ce message."
    ),
    email_verified_ok: "Email vérifié!",
    email_verified_error: "Ce lien de vérification d'email ne semble pas valide…",
    email_unsubscribed_ok: "C'est bon, nous n'enverrons plus d'emails!",
    email_unsubscribed_error: "Ce lien de désabonnement d'email ne semble pas valide…",
    email_you_can_close: "Tu peux maintenant fermer cette fenêtre.",
    month_formatter: &MONTH_FORMATTER_FR,
    month_day_formatter: &MONTH_DAY_FORMATTER_FR,
    year_month_day_formatter: &YEAR_MONTH_DAY_FORMATTER_FR,
};

static EN: Strings = Strings {
    lang: "en",
    index_hint: "Hover a day to add a booking or view its details",
    index_hint_touch: "Tap a day to add a booking or view its details",
    index_hint_end_day: "Click the last day of your stay",
    index_hint_end_day_touch: "Tap the last day of your stay",
    index_hint_booking_complete_info: "Booking saved!",
    index_hint_booking_notifications_cta: "Get notified if someone books on this period",
    index_hint_booking_notifications_pending: "Check your email, a verification link is waiting for you",
    index_hint_booking_notifications_active: "You will be notified if someone books on this period",
    modal_close: "Close",
    day_popover_empty: "No bookings",
    day_popover_edit_button_title: "Edit",
    day_popover_delete_button_title: "Delete",
    day_at_capacity: "At capacity",
    day_popover_confirm_delete_message: "Are you sure you want to delete this booking?",
    guest_one: "guest",
    guest_many: "guests",
    form_name_placeholder: "Name",
    form_add: "Add",
    form_saving: "Saving…",
    internal_error: "Internal server error",
    name_empty: "The name cannot be empty",
    booking_end_before_start: "The end date is before the start date",
    booking_guest_count_zero: "At least one guest is required",
    unknown_person: "Unknown person",
    start_booking: "Book…",
    booking_modal_title_new: "New booking",
    booking_modal_title_edit: "Edit booking",
    booking_modal_dates: "Dates",
    booking_modal_guest_count: "Guests",
    booking_modal_save: "Save",
    name_modal_title: "What's your name?",
    name_modal_name: "Name",
    name_modal_save: "Save",
    email_modal_title: "What's your email?",
    email_modal_name: "Email",
    email_modal_save: "Save",
    profile_login: "Login",
    profile_notifications_status: "Notifications",
    profile_notifications_status_none: "disabled",
    profile_notifications_status_pending: "pending verification",
    profile_notifications_status_active: "enabled",
    profile_notifications_status_disabled: "disabled",
    profile_notifications_enable: "Enable",
    profile_notifications_resend_verification: "Resend verification email",
    profile_notifications_disable: "Disable",
    profile_disconnect: "Disconnect",
    log_booking_created_title_suffix: "added a booking",
    log_booking_changed_title_suffix: "changed a booking",
    log_booking_deleted_title_suffix: "deleted a booking",
    navbar_link_calendar: "Bookings",
    navbar_link_log: "Log",
    notification_greeting: "Hello {recipient_name}!",
    notification_will_stay_with_you_subject: "New booking on your dates",
    notification_will_stay_with_you_body: concat!(
        "{booker_name} just added a new booking for {guest_count} people and will",
        " stay with you from {start_date} to {end_date}.",
    ),
    notification_will_not_stay_with_you_anymore_subject: "Booking deleted on your dates",
    notification_will_not_stay_with_you_anymore_body: "{booker_name} has deleted their booking for {guest_count} people from {start_date} to {end_date}.",
    notification_signature: "Greetings, the booking system",
    notification_unsubscribe: "Click the link below to unsubscribe:",
    verification_email_subject: "Verify your email",
    verification_email_body: concat!(
        "Click the link below to verify your email address:\n\n{verification_link}\n\n",
        "If you didn't request this, you can safely ignore this message."
    ),
    email_verified_ok: "Email verified!",
    email_verified_error: "This verification link does not seem valid…",
    email_unsubscribed_ok: "All good, we won't send emails anymore!",
    email_unsubscribed_error: "This unsubscription link does not seem valid…",
    email_you_can_close: "You can now close this window.",
    month_formatter: &MONTH_FORMATTER_EN,
    month_day_formatter: &MONTH_DAY_FORMATTER_EN,
    year_month_day_formatter: &YEAR_MONTH_DAY_FORMATTER_EN,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_empty_accept_language_falls_back_to_english() {
        assert_eq!(Locale::from_accept_language(""), Locale::En);
        assert_eq!(Locale::from_accept_language("de,it;q=0.8"), Locale::En);
        assert_eq!(Locale::from_accept_language("*"), Locale::En);
    }

    #[test]
    fn region_subtags_and_casing_are_ignored() {
        assert_eq!(Locale::from_accept_language("en-GB"), Locale::En);
        assert_eq!(Locale::from_accept_language("FR-ch"), Locale::Fr);
    }

    #[test]
    fn highest_quality_wins_regardless_of_order() {
        assert_eq!(
            Locale::from_accept_language("en;q=0.5,fr;q=0.9"),
            Locale::Fr
        );
        assert_eq!(
            Locale::from_accept_language("fr;q=0.5,en;q=0.9"),
            Locale::En
        );
        assert_eq!(
            Locale::from_accept_language("de,en;q=0.7,fr;q=0.3"),
            Locale::En
        );
    }

    #[test]
    fn equal_quality_keeps_the_first_match() {
        assert_eq!(Locale::from_accept_language("en,fr"), Locale::En);
        assert_eq!(Locale::from_accept_language("fr,en"), Locale::Fr);
    }
}
