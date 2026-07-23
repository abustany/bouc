use std::collections::HashMap;
use std::sync::LazyLock;

use icu::calendar::{Date as IcuDate, Iso};
use icu::datetime::DateTimeFormatter;
use icu::datetime::fieldsets::{M, MD};
use icu::locale::locale;
use jiff::civil::date;
use jiff_icu::ConvertInto;
use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::bookings::{BookingId, Person, PersonId};
use crate::strings::Locale;

const APP_TITLE: &str = "Bouc 🐏";

pub fn layout(locale: Locale, children: Markup) -> Markup {
    let s = locale.strings();

    html! {
        (DOCTYPE)
        html lang=(s.lang) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (APP_TITLE) }
                link rel="stylesheet" href="/assets/styles.css";
                script src="/assets/vendor/htmx-2.0.10.min.js" {};
                script src="/assets/day-popover.js" defer {};
            }
            body
              .grid .grid-flow-row .px-4 .py-2 .justify-center
            {
                h1 .text-xl .text-center .mb-2 { (APP_TITLE) }
                (children)
            }
        }
    }
}

pub struct IndexOpts<'a, 'b> {
    pub locale: Locale,
    pub start_year: i16,
    pub start_month: i8,
    pub sorted_bookings: &'a [Booking],
    pub max_capacity: u32,
    pub people: &'b HashMap<PersonId, Person>,
}

pub const CALENDARS_ELEMENT_ID: &str = "calendars";

pub fn index(opts: IndexOpts) -> Markup {
    let mut cals = vec![(opts.start_year, opts.start_month)];
    let mut cur_year = opts.start_year;
    let mut cur_month = opts.start_month;

    for _ in 1..12 {
        cur_month += 1;
        if cur_month > 12 {
            cur_year += 1;
            cur_month = 1;
        }
        cals.push((cur_year, cur_month));
    }

    let s = opts.locale.strings();

    layout(
        opts.locale,
        html! {
            p .pb-2 .text-center .italic { (opts.locale.strings().index_hint) }
            (calendars(&CalendarsOpts {
                locale: opts.locale,
                id: Some(CALENDARS_ELEMENT_ID.to_string()),
                start_year: opts.start_year,
                start_month: opts.start_month,
                sorted_bookings: opts.sorted_bookings,
                max_capacity: opts.max_capacity,
                people: opts.people,
                hx_swap_oob: false,
            }))
            (modal(&ModalOpts {
                title: html! { (s.booking_modal_title_new) },
                children: booking_modal_contents(opts.locale),
                id: Some("booking-modal".to_string()),
                ..Default::default()
            }))
            script {
                "window.localizedStrings = "
                (PreEscaped(serde_json::to_string(&s).expect("error serializing localized strings to JSON")))
            }
        },
    )
}

pub struct CalendarsOpts<'a, 'b> {
    pub id: Option<String>,
    pub hx_swap_oob: bool,
    pub locale: Locale,
    pub start_year: i16,
    pub start_month: i8,
    pub sorted_bookings: &'a [Booking],
    pub max_capacity: u32,
    pub people: &'b HashMap<PersonId, Person>,
}

pub fn calendars(opts: &CalendarsOpts) -> Markup {
    let mut cals = vec![(opts.start_year, opts.start_month)];
    let mut cur_year = opts.start_year;
    let mut cur_month = opts.start_month;

    for _ in 1..12 {
        cur_month += 1;
        if cur_month > 12 {
            cur_year += 1;
            cur_month = 1;
        }
        cals.push((cur_year, cur_month));
    }

    html! {
        div
          id=[opts.id.clone()]
          hx-swap-oob=(opts.hx_swap_oob)
          data-calendars
          .grid
          .grid-cols-3
          ."max-[909px]:grid-cols-2"
          ."max-[609px]:grid-cols-1"
          .gap-4
          .justify-items-center
        {
            @for (year, month) in cals {
                (calendar(&CalendarOpts {
                    locale: opts.locale, year, month, sorted_bookings: opts.sorted_bookings, max_capacity: opts.max_capacity, people: opts.people,
                }))
            }
        }

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

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn month_name(locale: Locale, d: &jiff::civil::Date) -> String {
    let formatter = match locale {
        Locale::Fr => &MONTH_FORMATTER_FR,
        Locale::En => &MONTH_FORMATTER_EN,
    };
    let icu_date: IcuDate<Iso> = (*d).convert_into();
    ucfirst(&formatter.format(&icu_date).to_string())
}

static DAY_FORMATTER_FR: LazyLock<DateTimeFormatter<MD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("fr").into(), MD::long())
        .expect("failed to build French day formatter")
});

static DAY_FORMATTER_EN: LazyLock<DateTimeFormatter<MD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("en").into(), MD::long())
        .expect("failed to build English day formatter")
});

fn day_name(locale: Locale, d: &jiff::civil::Date) -> String {
    let formatter = match locale {
        Locale::Fr => &DAY_FORMATTER_FR,
        Locale::En => &DAY_FORMATTER_EN,
    };
    let icu_date: IcuDate<Iso> = (*d).convert_into();
    formatter.format(&icu_date).to_string()
}

#[derive(Debug)]
pub struct Booking {
    pub id: BookingId,
    pub start_date: jiff::civil::Date,
    pub end_date: jiff::civil::Date,
    pub guest_count: u32,
    pub creator_id: PersonId,
}

pub struct CalendarOpts<'a, 'b> {
    pub locale: Locale,
    pub year: i16,
    pub month: i8,
    pub sorted_bookings: &'a [Booking],
    pub max_capacity: u32,
    pub people: &'b HashMap<PersonId, Person>,
}

fn is_date_in_month(d: &jiff::civil::Date, year: i16, month: i8) -> bool {
    d.year() == year && d.month() == month
}

fn is_booking_in_month(b: &Booking, year: i16, month: i8) -> bool {
    is_date_in_month(&b.start_date, year, month) || is_date_in_month(&b.end_date, year, month)
}

fn filter_month_bookings<'a>(
    sorted_bookings: impl IntoIterator<Item = &'a Booking>,
    year: i16,
    month: i8,
) -> impl Iterator<Item = &'a Booking> {
    sorted_bookings
        .into_iter()
        .skip_while(move |b| !is_booking_in_month(b, year, month))
        .take_while(move |b| is_booking_in_month(b, year, month))
}

struct CalendarDay<'a, 'b> {
    day: i8,
    guest_count: u32,
    bookings: Vec<&'a Booking>,
    people: &'b HashMap<PersonId, Person>,
}

fn calendar(opts: &CalendarOpts) -> Markup {
    let first_month_day = date(opts.year, opts.month, 1);
    let last_month_day = first_month_day.last_of_month();
    let mut days = (1..=first_month_day.days_in_month())
        .map(|day| CalendarDay {
            day,
            guest_count: 0,
            bookings: Vec::new(),
            people: opts.people,
        })
        .collect::<Vec<_>>();
    for b in filter_month_bookings(opts.sorted_bookings, opts.year, opts.month) {
        let start_in_month = std::cmp::max(b.start_date, first_month_day);
        let end_in_month = std::cmp::min(b.end_date, last_month_day);

        for d in start_in_month.day()..=end_in_month.day() {
            let day_index: usize = (d - 1).try_into().unwrap();
            days[day_index].guest_count += b.guest_count;
            days[day_index].bookings.push(b);
        }
    }

    html! {
        div .grid .grid-flow-row .content-start {
            p .text-center .font-semibold {(month_name(opts.locale, &first_month_day)) " " (opts.year)}
            div
            .grid ."grid-cols-[repeat(7,2.5rem)]"
            {
                @for CalendarDay{day, guest_count, bookings, people} in days {
                    @let style = if day == 1 { Some(format!("grid-column-start: {};", first_month_day.weekday().to_monday_one_offset())) } else { None };
                    div
                      data-day=(format!("{:04}{:02}{:02}", opts.year, opts.month, day))
                      .grid
                      ."border-2"
                      ."border-transparent"
                      ."border-b-amber-300"[guest_count > 0]
                      ."border-b-red-600"[guest_count >= opts.max_capacity]
                      style=[style]
                    {

                      button
                        .grid
                        .place-content-center
                        ."h-[2.5rem]"
                        ."w-[2.5rem]"
                        .rounded-full
                        ."hover:bg-border"
                        type="button"
                      { (day) }

                      (day_popover(opts.locale, date(opts.year, opts.month, day), &bookings, people))
                    }
                }
            }
        }
    }
}

fn date_to_yyyymmdd(d: &jiff::civil::Date) -> String {
    d.strftime("%Y%m%d").to_string()
}

fn day_popover(
    locale: Locale,
    day: jiff::civil::Date,
    bookings: &[&Booking],
    people: &HashMap<PersonId, Person>,
) -> Markup {
    let s = locale.strings();

    html! {
        // the vertical padding is the visual gap to the day cell, kept inside the
        // popover so the pointer never leaves the cell on its way here
        div
          data-day-popover
          hidden
          .fixed
          ."z-[1000]"
          ."w-max"
          ."max-w-[30rem]"
          ."py-1"
        {
            div
              .grid
              .grid-flow-row
              .gap-1
              .text-left
              .text-sm
              ."px-3"
              ."py-2"
              .rounded-md
              .border
              .border-border
              ."bg-white"
              ."shadow-lg"
            {
                p .font-semibold .text-center { (day_name(locale, &day)) }

                @if bookings.is_empty() {
                    p .italic { (s.day_popover_empty) }
                } @else {
                    ul .grid ."gap-1" {
                        @for b in bookings {
                            @let creator_name = people.get(&b.creator_id).map(|p| p.name.as_str()).unwrap_or(s.unknown_person);
                            li {
                                (day_name(locale, &b.start_date))
                                " → "
                                (day_name(locale, &b.end_date))
                                " · "
                                (creator_name)
                                " ("
                                (s.guests(b.guest_count))
                                ") "
                                button
                                  data-edit-booking
                                  data-booking-id=(u32::from(b.id))
                                  data-start-date=(date_to_yyyymmdd(&b.start_date))
                                  data-end-date=(date_to_yyyymmdd(&b.end_date))
                                  data-name=(creator_name)
                                  data-guest-count=(b.guest_count)
                                  title=(s.day_popover_edit_button_title)
                                  .cursor-pointer
                                {
                                    "✏️"
                                }
                                " "
                                button
                                  data-delete-booking
                                  data-booking-id=(u32::from(b.id))
                                  title=(s.day_popover_delete_button_title)
                                  hx-confirm=(s.day_popover_confirm_delete_message)
                                  hx-delete={"/bookings/" (u32::from(b.id))}
                                  hx-swap="none" // server will OOB-swap the calendars
                                  .cursor-pointer
                                {
                                    "🗑️"
                                }
                            }
                        }
                    }
                }

                button
                  data-start-booking
                  .justify-self-center
                  .btn-primary
                {
                    (s.start_booking)
                }
            }
        }
    }
}

#[derive(Default)]
struct ModalOpts {
    locale: Locale,
    title: Markup,
    children: Markup,
    id: Option<String>,
}

fn modal(opts: &ModalOpts) -> Markup {
    html! {
        dialog
          .w-full ."max-w-[600px]" .m-auto ."bg-white" ."px-4" ."py-2" ."rounded-2xl" ."backdrop:bg-black/50"
          id=[opts.id.clone()]
          data-modal
        {
            // display utilities on the dialog itself would override the user
            // agent's display:none for the closed state
            div .grid .grid-flow-row {
                div .grid ."grid-cols-[1fr_auto]" .gap-2 .mb-1 {
                    div data-title .font-semibold { (opts.title) }
                    button
                      data-close-button
                      aria-label=(opts.locale.strings().modal_close)
                    { "×" }
                }
                (opts.children)
            }
        }
    }
}

fn booking_modal_contents(locale: Locale) -> Markup {
    let s = locale.strings();

    html! {
        form
          .grid .grid-flow-row .gap-1 .my-1
          method="post"
          action="/bookings"
          hx-post="/bookings"
          hx-swap="none" // server will OOB-swap the calendars
        {
            div {
                (s.booking_modal_dates)
                ": "
                span data-start-date {}
                " → "
                span data-end-date {}
            }
            label .grid ."grid-cols-[auto_1fr]" .items-center .gap-1 {
                (s.booking_modal_name)
                ": "
                input type="text" name="name" required {}
            }
            label .grid ."grid-cols-[auto_1fr]" .items-center .gap-1 {
                (s.booking_modal_guest_count)
                ": "
                input type="number" name="guest_count" min="1" value="1" {}
            }
            input type="hidden" name="id" {}
            input type="hidden" name="start_date" {}
            input type="hidden" name="end_date" {}
            div .grid .justify-end .mt-2 {
                button .btn-primary {
                    (s.booking_modal_save)
                }
            }
        }
    }
}
