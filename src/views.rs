use std::collections::HashMap;

use jiff::Zoned;
use jiff::civil::date;
use jiff::tz::TimeZone;
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde::Serialize;

use crate::bookings::{Booking, BookingLogEntry, BookingLogEntryPayload, Person, PersonId};
use crate::strings::Locale;

pub const LOGGED_IN_INFO_ELEMENT_ID: &str = "logged-in-info";

struct SwitcherButtonOpts<'a, 'b> {
    href: &'a str,
    label: &'b str,
    active: bool,
}

fn switcher_button(opts: &SwitcherButtonOpts<'_, '_>) -> Markup {
    html! {
        a
          aria-current=[opts.active.then_some("page")]
          .py-1 .px-2 .border .rounded-full .border-transparent .border-gray-200[opts.active] .bg-white[opts.active] href=(opts.href)
        { (opts.label) }
    }
}

#[derive(Clone, PartialEq)]
pub enum ActivePage {
    Calendar,
    Log,
}

fn skeleton(lang: &str, children: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang=(lang) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Bouc 🐏" }
                link rel="stylesheet" href="/assets/styles.css";
                script src="/assets/vendor/htmx-2.0.10.min.js" defer {};
                script src="/assets/index.js" defer {};
                script src="/assets/vendor/alpine-3.15.12.min.js" defer {};
            }
            (children)
        }
    }
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NotificationSubscriptionState {
    None,
    Pending,
    Active,
    Disabled,
}

pub struct LayoutOpts<'a> {
    locale: Locale,
    people: &'a HashMap<PersonId, Person>,
    user_id: Option<PersonId>,
    notification_subscription_state: NotificationSubscriptionState,
    active_page: ActivePage,
}

pub fn layout(opts: &LayoutOpts, children: Markup) -> Markup {
    let s = opts.locale.strings();
    let user_id_js_str = serde_json::to_string(&opts.user_id.map(|id| u32::from(id).to_string()))
        .expect("error encoding user id to json");
    let notification_subscription_state_str =
        serde_json::to_string(&opts.notification_subscription_state)
            .expect("error encoding has active notification subscription to json");

    skeleton(
        s.lang,
        html! {
            body
              x-data={"app(" (user_id_js_str) ", " (notification_subscription_state_str) ")"}
              x-on:user-logged-in="onUserLoggedIn($event.detail.userId, $event.detail.notificationSubscriptionState)"
              x-on:user-logged-out="onUserLoggedOut()"
              x-on:notification-subscription-state-changed="onNotificationSubscriptionStateChanged($event.detail.state)"
              .grid .grid-cols-1 .px-4 .py-2 .mt-14
            {
                header
                  .fixed .top-0 .inset-x-0 .bg-white .shadow-lg
                  .grid ."grid-cols-[minmax(auto,1fr)_auto]" .gap-2 .items-center .py-1 .px-2
                {
                    h1 .text-xl .text-center .flex .flex-row .gap-1 { span .hidden .md:block { "Bouc" } span { "🐏" } }
                    div
                      .grid .grid-flow-col .items-center .gap-2 .lg:gap-4
                    {
                        div .grid .grid-flow-col .items-center .bg-gray-300 ."py-0.5" ."px-0.5" .rounded-full {
                            (switcher_button(&SwitcherButtonOpts { href: "/", label: &format!("🗓️ {}", s.navbar_link_calendar), active: opts.active_page == ActivePage::Calendar }))
                            (switcher_button(&SwitcherButtonOpts { href: "/log", label: &format!("📕 {}", s.navbar_link_log), active: opts.active_page == ActivePage::Log }))
                        }
                        (logged_in_info(&LoggedInInfoOpts {
                            id: Some(LOGGED_IN_INFO_ELEMENT_ID.to_string()),
                            hx_swap_oob: false,
                            locale: opts.locale,
                            people: opts.people,
                            user_id: opts.user_id,
                            notification_subscription_state: opts.notification_subscription_state,
                        }))
                    }
                }
                (children)
                (email_modal(&EmailModalOpts {
                    locale: opts.locale,
                }))
                script {
                    "window.localizedStrings = "
                    (PreEscaped(serde_json::to_string(&s).expect("error serializing localized strings to JSON")))
                }
            }

        },
    )
}

pub struct IndexOpts<'a, 'b> {
    pub locale: Locale,
    pub start_year: i16,
    pub start_month: i8,
    pub sorted_bookings: &'a [Booking],
    pub max_capacity: u32,
    pub people: &'b HashMap<PersonId, Person>,
    pub user_id: Option<PersonId>,
    pub notification_subscription_state: NotificationSubscriptionState,
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
        &LayoutOpts {
            active_page: ActivePage::Calendar,
            locale: opts.locale,
            people: opts.people,
            user_id: opts.user_id,
            notification_subscription_state: opts.notification_subscription_state,
        },
        html! {
            div
              x-data="calendar"
              "x-on:keydown.escape.window"="closePopover()"
              "x-on:booking-saved.window"="onBookingSaved()"
            {
                p .pb-2 .text-center .italic .min-h-8 role="region" aria-label="Notifications" {
                    span x-show="hintToShow === 'pick-day'" {
                        (pointer_touch_switch(s.index_hint, s.index_hint_touch))
                    }
                    span x-cloak x-show="hintToShow === 'pick-end-day'" {
                        (pointer_touch_switch(s.index_hint_end_day, s.index_hint_end_day_touch))
                    }
                    span x-cloak x-show="hintToShow === 'booking-complete'" .space-x-2 {
                        span { (s.index_hint_booking_complete_info) }

                        button
                            x-cloak x-show="notificationSubscriptionState === 'none'"
                            "x-on:click.self"={"onSubscribeToNotifications()"}
                            .underline .underline-offset-4 .cursor-pointer
                        {
                            "✉️ " (s.index_hint_booking_notifications_cta)
                        }

                        span x-cloak x-show="notificationSubscriptionState === 'pending'" { (s.index_hint_booking_notifications_pending) }

                        span x-cloak x-show="notificationSubscriptionState === 'active'" { (s.index_hint_booking_notifications_active) }
                    }
                }


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
            }
            (booking_modal(&BookingModalOpts {
                locale: opts.locale,
            }))
            (name_modal(&NameModalOpts {
                locale: opts.locale,
            }))
        },
    )
}

fn pointer_touch_switch(pointer: &str, touch: &str) -> Markup {
    html! {
        span ."sheet:hidden" { (pointer) }
        span .hidden ."sheet:block" { (touch) }
    }
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
          role="region"
          aria-label="Calendars"
          hx-swap-oob=(opts.hx_swap_oob)
          .grid
          // a month is 7 day cells of 2.5rem, the max width fits 3 of them
          // plus their gaps and stops the grid from growing a 4th column
          ."grid-cols-[repeat(auto-fit,17.5rem)]"
          ."max-w-[54.5rem]"
          .mx-auto
          .justify-center
          .gap-4
        {
            @for (year, month) in cals {
                (calendar(&CalendarOpts {
                    locale: opts.locale, year, month, sorted_bookings: opts.sorted_bookings, max_capacity: opts.max_capacity, people: opts.people,
                }))
            }
        }

    }
}

pub struct CalendarOpts<'a, 'b> {
    pub locale: Locale,
    pub year: i16,
    pub month: i8,
    pub sorted_bookings: &'a [Booking],
    pub max_capacity: u32,
    pub people: &'b HashMap<PersonId, Person>,
}

struct CalendarDay<'a, 'b> {
    day: i8,
    guest_count: u32,
    bookings: Vec<&'a Booking>,
    people: &'b HashMap<PersonId, Person>,
}

/// Bookings must be sorted by start date: the iteration stops at the first
/// booking starting after the month.
fn month_days<'a, 'b>(
    sorted_bookings: &'a [Booking],
    people: &'b HashMap<PersonId, Person>,
    first_month_day: jiff::civil::Date,
) -> Vec<CalendarDay<'a, 'b>> {
    let last_month_day = first_month_day.last_of_month();
    let mut days = (1..=first_month_day.days_in_month())
        .map(|day| CalendarDay {
            day,
            guest_count: 0,
            bookings: Vec::new(),
            people,
        })
        .collect::<Vec<_>>();

    for b in sorted_bookings
        .iter()
        .take_while(|b| b.start_date <= last_month_day)
        .filter(|b| b.end_date >= first_month_day)
    {
        let start_in_month = std::cmp::max(b.start_date, first_month_day);
        let end_in_month = std::cmp::min(b.end_date, last_month_day);

        for d in start_in_month.day()..=end_in_month.day() {
            let day_index: usize = (d - 1).try_into().unwrap();
            days[day_index].guest_count += b.guest_count;
            days[day_index].bookings.push(b);
        }
    }

    days
}

fn calendar(opts: &CalendarOpts) -> Markup {
    let first_month_day = date(opts.year, opts.month, 1);
    let days = month_days(opts.sorted_bookings, opts.people, first_month_day);
    let month_name_id = format!("month-name-{}", date_to_yyyymmdd(&first_month_day));

    html! {
        div
          .grid .grid-flow-row .content-start
          aria-labelledby=(month_name_id)
        {
            p id=(month_name_id) .text-center .font-semibold {(opts.locale.strings().format_date_month_name(first_month_day)) " " (opts.year)}
            div
            .grid ."grid-cols-[repeat(7,2.5rem)]"
            {
                @for CalendarDay{day, guest_count, bookings, people} in days {
                    @let style = if day == 1 { Some(format!("grid-column-start: {};", first_month_day.weekday().to_monday_one_offset())) } else { None };
                    @let date = date(opts.year, opts.month, day);
                    @let yyyymmdd = date_to_yyyymmdd(&date);
                    @let day_name = opts.locale.strings().format_date_month_day(date);
                    @let booking_status = if guest_count == 0 {
                        opts.locale.strings().day_popover_empty.to_owned()
                    } else if guest_count >= opts.max_capacity {
                        format!("{}, {}", opts.locale.strings().guests(guest_count), opts.locale.strings().day_at_capacity)
                    } else {
                        opts.locale.strings().guests(guest_count)
                    };
                    @let day_label = format!("{day_name}: {booking_status}");
                    div
                      .grid
                      ."border-2"
                      ."border-transparent"
                      ."border-b-amber-300"[guest_count > 0]
                      ."border-b-red-600"[guest_count >= opts.max_capacity]
                      style=[style]
                      x-on:mouseenter={"onDayMouseEnter('" (yyyymmdd) "')"}
                      x-on:focusin={"onDayMouseEnter('" (yyyymmdd) "')"}
                      x-on:mouseleave="onDayMouseLeave()"
                      x-ref={"cell-" (yyyymmdd)}
                    {

                      time datetime=(date.strftime("%F")) {
                          button
                            aria-label=(day_label)
                            .grid
                            .place-content-center
                            ."h-[2.5rem]"
                            ."w-[2.5rem]"
                            .rounded-full
                            ."hover:bg-border"
                            "x-on:click.self"={"onDayClick('" (yyyymmdd) "')"}
                            ":class"={"pendingBookingDayClass('" (yyyymmdd) "')"}
                            type="button"
                          { (day) }
                      }

                      (day_popover(opts.locale, date, &bookings, people))
                    }
                }
            }
        }
    }
}

fn date_to_yyyymmdd(d: &jiff::civil::Date) -> String {
    d.strftime("%Y%m%d").to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsBooking {
    id: String,
    start_day: String,
    end_day: String,
    name: String,
    guest_count: u32,
}

fn get_person_name(
    locale: Locale,
    people: &HashMap<PersonId, Person>,
    person_id: PersonId,
) -> &str {
    people
        .get(&person_id)
        .map(|p| p.name.as_str())
        .unwrap_or(locale.strings().unknown_person)
}

fn day_popover(
    locale: Locale,
    day: jiff::civil::Date,
    bookings: &[&Booking],
    people: &HashMap<PersonId, Person>,
) -> Markup {
    let s = locale.strings();
    let yyyymmdd = date_to_yyyymmdd(&day);
    let open_condition = format!("shouldShowPopoverForDay('{yyyymmdd}')");
    let display_day = s.format_date_month_day(day);
    let title_id = format!("day-popover-title-{yyyymmdd}");

    html! {
        // the vertical padding is the visual gap to the day cell, kept inside the
        // popover so the pointer never leaves the cell on its way here
        div
          role="dialog"
          aria-labelledby=(title_id)
          ":aria-hidden"={"(!" (open_condition) ").toString()"}
          x-cloak
          ":class"={"popoverClasses('" (yyyymmdd) "')"}
          x-effect={(open_condition) " && placePopover('" (yyyymmdd) "')"}
          "x-on:scroll.window.passive"={(open_condition) " && placePopover('" (yyyymmdd) "')"}
          x-ref={"day-popover-" (yyyymmdd)}
          .fixed
          ."z-[1000]"
          ."w-max"
          ."max-w-[min(90vw,30rem)]"
          ."py-1"
          ."sheet:inset-x-0"
          ."sheet:top-auto"
          ."sheet:bottom-0"
          ."sheet:w-auto"
          ."sheet:max-w-none"
          ."sheet:py-0"
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
              ."sheet:text-base"
              ."sheet:px-4"
              ."sheet:pt-4"
              ."sheet:pb-[calc(1rem+env(safe-area-inset-bottom))]"
              ."sheet:shadow-sheet"
              ."sheet:rounded-b-none"
              ."sheet:rounded-t-2xl"
              ."sheet:border-x-0"
              ."sheet:border-b-0"
              ."sheet:max-h-[70vh]"
              ."sheet:overflow-y-auto"
            {
                // the close button shares its grid cell with the title, which
                // keeps the title centered on the whole sheet
                div .grid .items-center {
                    p id=(title_id) .font-semibold .text-center ."col-start-1" ."row-start-1" {
                        time datetime=(day.strftime("%F")) { (display_day) }
                    }

                    button
                      .hidden
                      ."sheet:grid"
                      ."col-start-1"
                      ."row-start-1"
                      .justify-self-end
                      .place-content-center
                      .rounded-full
                      .size-8
                      ."bg-gray-100"
                      x-on:click="closePopover()"
                      aria-label=(s.modal_close)
                      type="button"
                    { "×" }
                }

                @if bookings.is_empty() {
                    p .italic { (s.day_popover_empty) }
                } @else {
                    ul .grid ."gap-1" {
                        @for b in bookings {
                            @let creator_name = get_person_name(locale, people, b.creator_id);
                            @let js_booking = JsBooking {
                                id: u32::from(b.id).to_string(),
                                start_day: date_to_yyyymmdd(&b.start_date),
                                end_day: date_to_yyyymmdd(&b.end_date),
                                name: creator_name.to_string(),
                                guest_count: b.guest_count,
                            };

                            li {
                                (s.format_date_month_day(b.start_date))
                                " → "
                                (s.format_date_month_day(b.end_date))
                                " · "
                                (creator_name)
                                " ("
                                (s.guests(b.guest_count))
                                ") "
                                button
                                  title=(s.day_popover_edit_button_title)
                                  aria-label=(s.day_popover_edit_button_title)
                                  x-cloak
                                  x-show={"userId === '" (u32::from(b.creator_id)) "'"}
                                  x-on:click={"editBooking(" (serde_json::to_string(&js_booking).expect("error serializing booking")) ")"}
                                  .cursor-pointer
                                {
                                    "✏️"
                                }
                                " "
                                button
                                  title=(s.day_popover_delete_button_title)
                                  aria-label=(s.day_popover_delete_button_title)
                                  x-cloak
                                  x-show={"userId === '" (u32::from(b.creator_id)) "'"}
                                  hx-confirm=(s.day_popover_confirm_delete_message)
                                  hx-delete={"/bookings/" (u32::from(b.id))}
                                  hx-swap="none" // server will OOB-swap the calendars
                                  x-on:click="justBooked = false"
                                  .cursor-pointer
                                {
                                    "🗑️"
                                }
                            }
                        }
                    }
                }

                button
                  x-on:click={"startBooking('" (yyyymmdd) "')"}
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
    x_ref: Option<String>,
    on_close: Option<String>,
}

fn modal(opts: &ModalOpts) -> Markup {
    let title_id = format!("modal-title-{:016x}", rand::random::<u64>());

    html! {
        dialog
          aria-labelledby=(title_id)
          .w-full ."max-w-[min(600px,calc(100vw-1rem))]" .m-auto ."bg-white" ."px-4" ."py-2" ."rounded-2xl" ."backdrop:bg-black/50"
          x-ref=[opts.x_ref.clone()]
          x-on:close=[opts.on_close.clone()]
        {
            // display utilities on the dialog itself would override the user
            // agent's display:none for the closed state
            div .grid .grid-flow-row {
                div .grid ."grid-cols-[1fr_auto]" .gap-2 .mb-1 {
                    div .font-semibold id=(title_id) { (opts.title) }
                    button
                      .grid .place-content-center .rounded-full .size-5 ."hover:bg-red-400"
                      x-on:click="$el.closest('dialog').close()"
                      aria-label=(opts.locale.strings().modal_close)
                    { "×" }
                }
                (opts.children)
            }
        }
    }
}

struct BookingModalOpts {
    locale: Locale,
}

fn booking_modal(opts: &BookingModalOpts) -> Markup {
    let s = opts.locale.strings();

    html! {
        div
          x-data="editBooking"
          x-on:booking-saved="onBookingSaved()"
        {
            (modal(&ModalOpts {
                title: html! { span x-text="modalTitle" { (s.booking_modal_title_new) }},
                children: booking_modal_contents(opts.locale),
                x_ref: Some("modal".to_string()),
                on_close: Some("onModalClose()".to_string()),
                locale: opts.locale,
            }))
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
                span x-text="booking ? formatDate(booking.startDay) : null" {}
                " → "
                span x-text="booking ? formatDate(booking.endDay) : null" {}
            }
            label .grid ."grid-cols-[auto_1fr]" .items-center .gap-1 {
                (s.booking_modal_guest_count)
                ": "
                input type="number" inputmode="numeric" name="guest_count" min="1" ":value"="booking?.guestCount" required {}
            }
            input type="hidden" name="id" ":value"="booking?.id" {}
            input type="hidden" name="start_date" ":value"="booking?.startDay" {}
            input type="hidden" name="end_date" ":value"="booking?.endDay" {}
            div .grid .justify-end .mt-2 {
                button .btn-primary {
                    (s.booking_modal_save)
                }
            }
        }
    }
}

struct NameModalOpts {
    locale: Locale,
}

fn name_modal(opts: &NameModalOpts) -> Markup {
    let s = opts.locale.strings();

    html! {
        div
          x-data="nameModal"
          "x-on:show-name-modal.window"="showModal()"
        {
            (modal(&ModalOpts {
                title: html! { (s.name_modal_title) },
                children: name_modal_contents(opts.locale),
                x_ref: Some("modal".to_string()),
                locale: opts.locale,
                ..Default::default()
            }))
        }
    }
}

fn name_modal_contents(locale: Locale) -> Markup {
    let s = locale.strings();

    html! {
        form
          .grid .grid-flow-row .gap-1 .my-1
          method="post"
          action="/login"
          hx-post="/login"
          hx-swap="none" // server will OOB-swap the profile header
        {
            label .grid ."grid-cols-[auto_1fr]" .items-center .gap-1 {
                (s.name_modal_name)
                ": "
                input type="text" name="name" required autofocus {}
            }
            div .grid .justify-end .mt-2 {
                button .btn-primary {
                    (s.name_modal_save)
                }
            }
        }

    }
}

struct EmailModalOpts {
    locale: Locale,
}

fn email_modal(opts: &EmailModalOpts) -> Markup {
    let s = opts.locale.strings();

    html! {
        div
          x-data="emailModal"
          "x-on:show-email-modal.window"="showModal()"
        {
            (modal(&ModalOpts {
                title: html! { (s.email_modal_title) },
                children: email_modal_contents(opts.locale),
                x_ref: Some("modal".to_string()),
                locale: opts.locale,
                ..Default::default()
            }))
        }
    }
}

fn email_modal_contents(locale: Locale) -> Markup {
    let s = locale.strings();

    html! {
        form
          .grid .grid-flow-row .gap-1 .my-1
          method="post"
          action="/notifications"
          hx-post="/notifications"
          hx-swap="none"
        {
            label .grid ."grid-cols-[auto_1fr]" .items-center .gap-1 {
                (s.email_modal_name)
                ": "
                input type="email" name="email" required autofocus {}
            }
            div .grid .justify-end .mt-2 {
                button .btn-primary {
                    (s.email_modal_save)
                }
            }
        }

    }
}

pub struct LoggedInInfoOpts<'a> {
    pub id: Option<String>,
    pub hx_swap_oob: bool,
    pub locale: Locale,
    pub people: &'a HashMap<PersonId, Person>,
    pub user_id: Option<PersonId>,
    pub notification_subscription_state: NotificationSubscriptionState,
}

pub fn logged_in_info(opts: &LoggedInInfoOpts) -> Markup {
    let s = opts.locale.strings();

    html! {
        div
          .grid ."grid-cols-[minmax(0,1fr)_auto]" .items-center .gap-1 .relative
          id=[opts.id.clone()]
          hx-swap-oob=(opts.hx_swap_oob)
        {
            @match opts.user_id {
                Some(user_id) => {
                    @let name = get_person_name(opts.locale, opts.people, user_id);

                    {
                        button
                          .truncate .underline .underline-offset-4 .cursor-pointer title=(name)
                          aria-label="Profile menu"
                          "x-on:click"="showProfileDropdown = !showProfileDropdown"
                        { "👤 " (name) }

                        div
                          role="dialog"
                          x-cloak
                          x-show="showProfileDropdown"
                          "x-on:click.outside"="showProfileDropdown = false"
                          .fixed .grid .right-0 .top-12 .w-fit ."max-w-[95vw]" .px-2 .py-1 .bg-white .shadow-lg .rounded-lg
                        {
                            form
                              method="post"
                              action="/notifications"
                              hx-post="/notifications"
                              hx-swap="none"
                            {
                                @let (status,  button_label, disable_value) = match opts.notification_subscription_state {
                                    NotificationSubscriptionState::None => (s.profile_notifications_status_none, s.profile_notifications_enable, "0"),
                                    NotificationSubscriptionState::Pending => (s.profile_notifications_status_pending, s.profile_notifications_resend_verification, "0"),
                                    NotificationSubscriptionState::Active => (s.profile_notifications_status_active, s.profile_notifications_disable, "1"),
                                    NotificationSubscriptionState::Disabled => (s.profile_notifications_status_disabled, s.profile_notifications_enable, "0"),
                                };
                                @let (button_type, button_onclick) = match opts.notification_subscription_state {
                                    NotificationSubscriptionState::None | NotificationSubscriptionState::Disabled => ("button", Some("onSubscribeToNotifications()")),
                                    NotificationSubscriptionState::Pending | NotificationSubscriptionState::Active => ("submit", None),
                                };

                                input type="hidden" name="disable" value=(disable_value) {}

                                "✉️ " (s.profile_notifications_status) ": " (status) " ("
                                button
                                  type=(button_type)
                                  "x-on:click"=[button_onclick]
                                  .cursor-pointer .underline .underline-offset-4
                                {
                                    (button_label)
                                }
                                ")"
                            }
                            button
                              .text-left .cursor-pointer .mt-2
                              aria-label=(s.profile_disconnect)
                              hx-post="/logout"
                            {
                                "⏻️ "
                                span
                                  .underline .underline-offset-4
                                {
                                    (s.profile_disconnect)
                                }
                            }
                        }
                    }
                }
                None => {
                    button
                    .btn-primary
                    x-on:click="ensureLoggedIn()"
                    {
                        (s.profile_login)
                    }
                }
            }
        }
    }
}

struct BookingLogItemOpts {
    date: String,
    title: String,
    details: Markup,
}

impl BookingLogItemOpts {
    fn format_booking_details(locale: Locale, b: &Booking) -> String {
        let s = locale.strings();
        format!(
            "{} → {}, {}",
            s.format_date_month_day(b.start_date),
            s.format_date_month_day(b.end_date),
            s.guests(b.guest_count)
        )
    }

    fn from_booking_log_entry(
        locale: Locale,
        tz: TimeZone,
        people: &HashMap<PersonId, Person>,
        e: &BookingLogEntry,
    ) -> Self {
        let s = locale.strings();
        let actor_name = get_person_name(locale, people, e.creator_id).to_string();
        let date = s.format_date_month_day(<Zoned as Into<jiff::civil::Date>>::into(
            e.create_time.to_zoned(tz),
        ));

        match &e.payload {
            BookingLogEntryPayload::BookingCreated { booking } => Self {
                date,
                title: s.log_booking_created_title(&actor_name),
                details: html! { (Self::format_booking_details(locale, booking)) },
            },
            BookingLogEntryPayload::BookingChanged { before, after } => Self {
                date,
                title: s.log_booking_changed_title(&actor_name),
                details: html! {
                    p { "Before: " (Self::format_booking_details(locale, before)) }
                    p { "After: " (Self::format_booking_details(locale, after)) }
                },
            },
            BookingLogEntryPayload::BookingDeleted { booking } => Self {
                date,
                title: s.log_booking_deleted_title(&actor_name),
                details: html! { (Self::format_booking_details(locale, booking)) },
            },
        }
    }
}

fn booking_log_item(opts: &BookingLogItemOpts) -> Markup {
    html! {
        article .grid ."grid-cols-[7rem_minmax(0,1fr)]" .gap-x-2 {
            time .text-right {(&opts.date)}
            h2 .border-l-2 .border-l-gray-300 .pl-2 {(&opts.title)}
            div .col-start-2 .flex .flex-col .border-l-2 .border-l-gray-300 .pl-2 .italic .text-sm .pb-2 {(&opts.details)}
        }
    }
}

pub struct BookingLogOpts<'a, 'b> {
    pub locale: Locale,
    pub tz: TimeZone,
    pub people: &'a HashMap<PersonId, Person>,
    pub user_id: Option<PersonId>,
    pub notification_subscription_state: NotificationSubscriptionState,
    pub log_entries: &'b [BookingLogEntry],
}

pub fn booking_log(opts: &BookingLogOpts) -> Markup {
    layout(
        &LayoutOpts {
            active_page: ActivePage::Log,
            locale: opts.locale,
            people: opts.people,
            user_id: opts.user_id,
            notification_subscription_state: opts.notification_subscription_state,
        },
        html! {
            div
              role="region"
              aria-label="Booking log"
              .flex .flex-col .w-max .mx-auto
            {
                @for e in opts.log_entries {
                    (booking_log_item(&BookingLogItemOpts::from_booking_log_entry(opts.locale, opts.tz.clone(), opts.people, e)))
                }
            }
        },
    )
}

pub fn email_verified(locale: Locale, verified: bool) -> Markup {
    let s = locale.strings();
    skeleton(
        s.lang,
        html! {
            body
              .fixed .inset-0 .grid .place-content-center
            {
                p .text-xl .text-center {
                    @if verified {
                        (s.email_verified_ok)
                    } @else {
                        (s.email_verified_error)
                    }
                }
                p .text-center {
                    (s.email_you_can_close)
                }
            }
        },
    )
}

pub fn email_unsubscribed(locale: Locale, unsubscribed: bool) -> Markup {
    let s = locale.strings();
    skeleton(
        s.lang,
        html! {
            body
              .fixed .inset-0 .grid .place-content-center
            {
                p .text-xl .text-center {
                    @if unsubscribed {
                        (s.email_unsubscribed_ok)
                    } @else {
                        (s.email_unsubscribed_error)
                    }
                }
                p .text-center {
                    (s.email_you_can_close)
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookings::BookingId;

    fn booking(id: u32, start: jiff::civil::Date, end: jiff::civil::Date, guests: u32) -> Booking {
        Booking {
            id: BookingId::new(id),
            start_date: start,
            end_date: end,
            creator_id: PersonId::new(1),
            guest_count: guests,
        }
    }

    fn guest_counts(bookings: &[Booking], year: i16, month: i8) -> Vec<u32> {
        let people = HashMap::new();
        month_days(bookings, &people, date(year, month, 1))
            .iter()
            .map(|d| d.guest_count)
            .collect()
    }

    #[test]
    fn booking_spanning_a_whole_month_fills_every_day() {
        let bookings = [booking(1, date(2026, 9, 10), date(2026, 11, 10), 2)];

        assert_eq!(guest_counts(&bookings, 2026, 10), vec![2; 31]);
        assert_eq!(guest_counts(&bookings, 2026, 9)[7..11], [0, 0, 2, 2]);
        assert_eq!(guest_counts(&bookings, 2026, 11)[9..12], [2, 0, 0]);
    }

    #[test]
    fn bookings_outside_the_month_are_ignored() {
        let bookings = [
            booking(1, date(2026, 8, 1), date(2026, 8, 31), 3),
            booking(2, date(2026, 11, 1), date(2026, 11, 2), 3),
        ];

        assert_eq!(guest_counts(&bookings, 2026, 10), vec![0; 31]);
    }

    #[test]
    fn bookings_touching_the_month_edges_are_included() {
        let bookings = [
            booking(1, date(2026, 9, 28), date(2026, 10, 1), 1),
            booking(2, date(2026, 10, 31), date(2026, 11, 2), 3),
        ];

        let counts = guest_counts(&bookings, 2026, 10);
        assert_eq!(counts[..2], [1, 0]);
        assert_eq!(counts[29..], [0, 3]);
    }

    #[test]
    fn a_gap_in_the_sorted_bookings_does_not_hide_later_ones() {
        let bookings = [
            booking(1, date(2026, 9, 1), date(2026, 10, 3), 1),
            booking(2, date(2026, 9, 2), date(2026, 9, 3), 1),
            booking(3, date(2026, 10, 5), date(2026, 10, 6), 4),
        ];

        let counts = guest_counts(&bookings, 2026, 10);
        assert_eq!(counts[..3], [1, 1, 1]);
        assert_eq!(counts[3..6], [0, 4, 4]);
    }

    #[test]
    fn overlapping_bookings_add_up() {
        let bookings = [
            booking(1, date(2026, 10, 1), date(2026, 10, 3), 2),
            booking(2, date(2026, 10, 3), date(2026, 10, 4), 5),
        ];

        assert_eq!(guest_counts(&bookings, 2026, 10)[..5], [2, 2, 7, 5, 0]);
    }
}
