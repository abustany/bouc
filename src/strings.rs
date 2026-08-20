use serde::Serialize;

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

    fn from_language_tag(tag: &str) -> Option<Self> {
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
    pub profile_login: &'static str,
    pub profile_disconnect: &'static str,
    pub log_booking_created_title_suffix: &'static str,
    pub log_booking_changed_title_suffix: &'static str,
    pub log_booking_deleted_title_suffix: &'static str,
    pub navbar_link_calendar: &'static str,
    pub navbar_link_log: &'static str,
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
}

static FR: Strings = Strings {
    lang: "fr",
    index_hint: "Pointer sur un jour pour entrer une réservation ou afficher les détails",
    index_hint_touch: "Toucher un jour pour entrer une réservation ou afficher les détails",
    index_hint_end_day: "Cliquer sur le dernier jour du séjour",
    index_hint_end_day_touch: "Toucher le dernier jour du séjour",
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
    name_modal_title: "Comment t'appelle-tu ?",
    name_modal_name: "Nom",
    name_modal_save: "Enregistrer",
    profile_login: "S'identifier",
    profile_disconnect: "Se déconnecter",
    log_booking_created_title_suffix: "a ajouté une réservation",
    log_booking_changed_title_suffix: "a modifié une réservation",
    log_booking_deleted_title_suffix: "a supprimé une réservation",
    navbar_link_calendar: "Réservations",
    navbar_link_log: "Journal",
};

static EN: Strings = Strings {
    lang: "en",
    index_hint: "Hover a day to add a booking or view its details",
    index_hint_touch: "Tap a day to add a booking or view its details",
    index_hint_end_day: "Click the last day of your stay",
    index_hint_end_day_touch: "Tap the last day of your stay",
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
    profile_login: "Login",
    profile_disconnect: "Disconnect",
    log_booking_created_title_suffix: "added a booking",
    log_booking_changed_title_suffix: "changed a booking",
    log_booking_deleted_title_suffix: "deleted a booking",
    navbar_link_calendar: "Bookings",
    navbar_link_log: "Log",
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
