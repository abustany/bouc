use std::ops::Deref;
use std::sync::Arc;
use std::{collections::HashMap, convert::Infallible};

use anyhow::Context;
use axum::extract::FromRef;
use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use axum_extra::extract::SignedCookieJar;
use axum_extra::extract::cookie::{Cookie, Key};
use jiff::tz::TimeZone;
use maud::Markup;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;

use crate::bookings::{
    self, Booking, BookingId, BookingInput, BookingLogEntry, ListBookingsFilter,
    NotificationSubscription, NotificationSubscriptionPayload, Person, PersonId,
};
use crate::bookings::{Repository, validate_person_name};
use crate::notify::{self, Notifier};
use crate::strings::Locale;
use crate::views::{self, CALENDARS_ELEMENT_ID};

struct InnerAppState {
    repo: Arc<dyn Repository>,
    notifier: Arc<Notifier>,
    signed_cookies_key: Key,
    timezone: TimeZone,
    max_capacity: u32,
}

#[derive(Clone)]
struct AppState(Arc<InnerAppState>);

impl Deref for AppState {
    type Target = InnerAppState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.0.signed_cookies_key.clone()
    }
}

const HX_TRIGGER: &str = "hx-trigger";

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub fn router(
    repo: Arc<dyn Repository>,
    signed_cookies_key: Key,
    timezone: TimeZone,
    max_capacity: u32,
    notifier: Arc<Notifier>,
) -> Router {
    Router::new()
        // start of app routes
        .route("/", get(index))
        .route("/log", get(booking_log))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/bookings", post(save_booking))
        .route("/bookings/{id}", delete(delete_booking))
        .route(
            "/notifications/unsubscribe/{user_id}/{token}",
            get(unsubscribe),
        )
        .route("/notifications/verify/{user_id}/{token}", get(verify_email))
        .route("/notifications", post(save_notification))
        // end of app routes, any route *after* the route_layer call below does
        // not get the Vary header properly set to accept-language.
        .route_layer(middleware::from_fn(set_vary_accept_language))
        .route("/assets/{*path}", get(serve_asset))
        .with_state(AppState(Arc::new(InnerAppState {
            repo,
            notifier,
            signed_cookies_key,
            timezone,
            max_capacity,
        })))
}

async fn set_vary_accept_language(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("accept-language"));
    response
}

async fn serve_asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_control = if path.starts_with("vendor/") {
        "public, max-age=31536000, immutable"
    } else {
        "max-age=0, must-revalidate"
    };

    let etag = format!("\"{}\"", hex::encode(file.metadata.sha256_hash()));

    let not_modified = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag);

    if not_modified {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::CACHE_CONTROL, cache_control),
                (header::ETAG, etag.as_str()),
            ],
        )
            .into_response();
    }

    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype()),
            (header::CACHE_CONTROL, cache_control),
            (header::ETAG, etag.as_str()),
        ],
        file.data,
    )
        .into_response()
}

impl<S: Send + Sync> FromRequestParts<S> for Locale {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|value| value.to_str().ok())
            .map(Locale::from_accept_language)
            .unwrap_or_default())
    }
}

async fn list_bookings(repo: &dyn Repository, now: &jiff::Zoned) -> anyhow::Result<Vec<Booking>> {
    repo.list_bookings(ListBookingsFilter::EndsAfter(jiff::civil::date(
        now.year(),
        now.month(),
        1,
    )))
    .await
    .context("listing bookings")
}

async fn list_people(
    repo: &dyn Repository,
) -> anyhow::Result<HashMap<bookings::PersonId, bookings::Person>> {
    Ok(repo
        .list_people()
        .await
        .context("listing people")?
        .into_iter()
        .map(|p| (p.id, p))
        .collect::<HashMap<_, _>>())
}

fn get_current_user_id(jar: &SignedCookieJar) -> Option<PersonId> {
    jar.get(USER_ID_COOKIE_NAME)
        .and_then(|c| c.value().parse::<u32>().ok())
        .map(PersonId::new)
}

async fn get_notification_subscription_state(
    repo: &dyn Repository,
    user_id: Option<PersonId>,
) -> anyhow::Result<views::NotificationSubscriptionState> {
    let Some(user_id) = user_id else {
        return Ok(views::NotificationSubscriptionState::None);
    };

    Ok(
        match repo
            .get_notification_subscription(user_id)
            .await
            .context("retrieving notification subscription")?
        {
            Some(NotificationSubscription {
                payload: NotificationSubscriptionPayload::Pending { .. },
                ..
            }) => views::NotificationSubscriptionState::Pending,
            Some(NotificationSubscription {
                payload: NotificationSubscriptionPayload::Active { .. },
                ..
            }) => views::NotificationSubscriptionState::Active,
            Some(NotificationSubscription {
                payload: NotificationSubscriptionPayload::Disabled,
                ..
            }) => views::NotificationSubscriptionState::Disabled,
            None => views::NotificationSubscriptionState::None,
        },
    )
}

async fn index(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    locale: Locale,
) -> Result<Markup, AppError> {
    let now = jiff::Zoned::now();
    let user_id = get_current_user_id(&jar);
    let notification_subscription_state = get_notification_subscription_state(&*app.repo, user_id)
        .await
        .context("getting active notification subscription")?;

    Ok(views::index(views::IndexOpts {
        locale,
        start_year: now.year(),
        start_month: now.month(),
        sorted_bookings: &list_bookings(&*app.repo, &now)
            .await
            .context("listing bookings")?,
        max_capacity: app.max_capacity,
        people: &list_people(&*app.repo).await.context("listing people")?,
        user_id,
        notification_subscription_state,
    }))
}

async fn booking_log(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    locale: Locale,
) -> Result<Markup, AppError> {
    let user_id = get_current_user_id(&jar);
    let notification_subscription_state = get_notification_subscription_state(&*app.repo, user_id)
        .await
        .context("getting active notification subscription")?;

    Ok(views::booking_log(&views::BookingLogOpts {
        locale,
        tz: app.timezone.clone(),
        people: &list_people(&*app.repo).await.context("listing people")?,
        user_id,
        notification_subscription_state,
        log_entries: app
            .repo
            .list_booking_log(None)
            .await
            .context("loading booking log")?
            .as_slice(),
    }))
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

const USER_ID_COOKIE_NAME: &str = "bouc-user-id";

#[derive(Deserialize)]
struct LoginForm {
    name: String,
}

async fn login(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    headers: HeaderMap,
    locale: Locale,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let Ok(name) = validate_person_name(&form.name) else {
        return Ok((StatusCode::BAD_REQUEST, "invalid name").into_response());
    };

    let user = app.repo.save_person(&name).await.context("saving user")?;
    let user_id_str = u32::from(user.id).to_string();
    let user_id_cookie = Cookie::build((USER_ID_COOKIE_NAME, user_id_str.clone()))
        .http_only(true)
        .build();
    let updated_jar = jar.add(user_id_cookie);
    let notification_subscription_state =
        get_notification_subscription_state(&*app.repo, Some(user.id))
            .await
            .context("getting active notification subscription")?;

    if is_htmx(&headers) {
        let logged_in_info = oob_logged_in_info(locale, Some(&user)).await?;
        Ok((
            [(
                HX_TRIGGER,
                serde_json::to_string(&json!({"user-logged-in": {"userId": &user_id_str, "notificationSubscriptionState": &notification_subscription_state}}))
                    .expect("error marshalling user-logged-in event data"),
            )],
            updated_jar,
            logged_in_info,
        )
            .into_response())
    } else {
        Ok((updated_jar, Redirect::to("/")).into_response())
    }
}

async fn logout(
    jar: SignedCookieJar,
    headers: HeaderMap,
    locale: Locale,
) -> Result<Response, AppError> {
    let updated_jar = jar.remove(Cookie::from(USER_ID_COOKIE_NAME));

    if is_htmx(&headers) {
        let logged_in_info = oob_logged_in_info(locale, None).await?;
        Ok((
            [(HX_TRIGGER, "user-logged-out")],
            updated_jar,
            logged_in_info,
        )
            .into_response())
    } else {
        Ok((updated_jar, Redirect::to("/")).into_response())
    }
}

async fn oob_logged_in_info(
    locale: Locale,
    current_user: Option<&Person>,
) -> Result<Markup, AppError> {
    let people: HashMap<PersonId, Person> = if let Some(u) = current_user {
        vec![(u.id, u.clone())].into_iter().collect()
    } else {
        HashMap::new()
    };

    Ok(views::logged_in_info(&views::LoggedInInfoOpts {
        id: Some(views::LOGGED_IN_INFO_ELEMENT_ID.to_string()),
        hx_swap_oob: true,
        locale,
        people: &people,
        user_id: current_user.map(|u| u.id),
    }))
}

#[derive(Deserialize)]
struct SaveBookingForm {
    id: Option<String>,
    start_date: String,
    end_date: String,
    guest_count: String,
}

async fn save_booking(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    headers: HeaderMap,
    locale: Locale,
    Form(form): Form<SaveBookingForm>,
) -> Result<Response, AppError> {
    let Some(creator_id) = get_current_user_id(&jar) else {
        return Ok((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    };

    let Ok(start_date) = form.start_date.parse::<jiff::civil::Date>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid start date").into_response());
    };

    let Ok(end_date) = form.end_date.parse::<jiff::civil::Date>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid end date").into_response());
    };

    let Ok(guest_count) = form.guest_count.parse::<u32>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid guest count").into_response());
    };

    let Ok(booking) = BookingInput::new(start_date, end_date, creator_id, guest_count) else {
        return Ok((StatusCode::BAD_REQUEST, "invalid booking").into_response());
    };

    let booking_id = if let Some(id) = form.id
        && !id.is_empty()
    {
        let Ok(val) = id.parse::<u32>() else {
            return Ok((StatusCode::BAD_REQUEST, "invalid booking id").into_response());
        };
        Some(BookingId::new(val))
    } else {
        None
    };

    let log_entry = match booking_id {
        Some(id) => match app.repo.update_booking(id, creator_id, &booking).await {
            Ok((_, entry)) => entry,
            Err(bookings::UpdateBookingError::NotFound) => {
                return Ok((StatusCode::NOT_FOUND, "booking not found").into_response());
            }
            Err(e) => return Err(AppError::from(e)),
        },
        None => {
            app.repo
                .create_booking(&booking)
                .await
                .context("creating booking")?
                .1
        }
    };

    notify_booking_changed(&app.notifier, log_entry);

    if is_htmx(&headers) {
        let calendars = oob_calendars(&*app.repo, locale).await?;
        Ok(([(HX_TRIGGER, "booking-saved")], calendars).into_response())
    } else {
        Ok(Redirect::to("/").into_response())
    }
}

async fn delete_booking(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    locale: Locale,
    Path(id): Path<u32>,
) -> Result<Response, AppError> {
    let Some(user_id) = get_current_user_id(&jar) else {
        return Ok((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    };

    let log_entry = match app.repo.delete_booking(BookingId::new(id), user_id).await {
        Ok(entry) => entry,
        Err(bookings::DeleteBookingError::NotFound) => {
            return Ok((StatusCode::NOT_FOUND, "booking not found").into_response());
        }
        Err(e) => return Err(AppError::from(e)),
    };

    notify_booking_changed(&app.notifier, log_entry);

    Ok(oob_calendars(&*app.repo, locale).await.into_response())
}

async fn unsubscribe(
    State(app): State<AppState>,
    locale: Locale,
    Path((user_id, token)): Path<(PersonId, String)>,
) -> Result<Markup, AppError> {
    let is_valid = matches!(
        app
        .repo
        .get_notification_subscription(user_id)
        .await
        .context("retrieving notification subscription")?
        .map(|s| s.payload),
        Some(NotificationSubscriptionPayload::Active {
            unsubscribe_token, ..
        }) if unsubscribe_token == token
    );

    if is_valid {
        app.repo
            .save_notification_subscription(&NotificationSubscription {
                person_id: user_id,
                payload: NotificationSubscriptionPayload::Disabled,
            })
            .await
            .context("saving notification subscription")?;
    }

    Ok(views::email_unsubscribed(locale, is_valid))
}

async fn verify_email(
    State(app): State<AppState>,
    locale: Locale,
    Path((user_id, token)): Path<(PersonId, String)>,
) -> Result<Markup, AppError> {
    let email = match app
        .repo
        .get_notification_subscription(user_id)
        .await
        .context("retrieving notification subscription")?
        .map(|s| s.payload)
    {
        Some(NotificationSubscriptionPayload::Pending {
            email,
            verification_token,
            ..
        }) if verification_token == token => Some(email),
        _ => None,
    };

    if let Some(ref email) = email {
        app.repo
            .save_notification_subscription(&NotificationSubscription {
                person_id: user_id,
                payload: NotificationSubscriptionPayload::Active {
                    email: email.clone(),
                    locale: locale.strings().lang.to_owned(),
                    unsubscribe_token: notify::generate_verification_token(),
                },
            })
            .await
            .context("saving notification subscription")?;
    }

    Ok(views::email_verified(locale, email.is_some()))
}

#[derive(Deserialize)]
struct SaveNotificationForm {
    email: String,
}

async fn save_notification(
    State(app): State<AppState>,
    jar: SignedCookieJar,
    headers: HeaderMap,
    locale: Locale,
    Form(form): Form<SaveNotificationForm>,
) -> Result<Response, AppError> {
    let Some(user_id) = get_current_user_id(&jar) else {
        return Ok((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    };

    let Ok(email) = form.email.parse::<lettre::Address>().map(|a| a.to_string()) else {
        return Ok((StatusCode::BAD_REQUEST, "invalid email").into_response());
    };

    let subscription_state = match app
        .repo
        .get_notification_subscription(user_id)
        .await
        .context("retrieving notification subscription")?
        .map(|s| s.payload)
    {
        Some(NotificationSubscriptionPayload::Pending {
            email,
            verification_token,
        }) => {
            app.notifier
                .send_verification_email(user_id, &email, locale, &verification_token)
                .await
                .context("resending verification email")?;
            "pending"
        }
        Some(NotificationSubscriptionPayload::Active { .. }) => {
            // do nothing
            "active"
        }
        Some(NotificationSubscriptionPayload::Disabled) | None => {
            let verification_token = notify::generate_verification_token();
            app.repo
                .save_notification_subscription(&NotificationSubscription {
                    person_id: user_id,
                    payload: NotificationSubscriptionPayload::Pending {
                        email: email.clone(),
                        verification_token: verification_token.clone(),
                    },
                })
                .await
                .context("saving notification subscription")?;
            app.notifier
                .send_verification_email(user_id, &email, locale, &verification_token)
                .await
                .context("sending verification email")?;
            "pending"
        }
    };

    if is_htmx(&headers) {
        Ok((
            [(
                HX_TRIGGER,
                serde_json::to_string(
                    &json!({"notification-subscription-state-changed": {"state": subscription_state}}),
                )
                .expect("error marshalling notification-subscription-state-changed event data"),
            )],
            StatusCode::OK,
        )
            .into_response())
    } else {
        Ok(Redirect::to("/").into_response())
    }
}

fn notify_booking_changed(notifier: &Arc<Notifier>, entry: BookingLogEntry) {
    let notifier = notifier.clone();
    tokio::spawn(async move {
        if let Err(err) = notifier.notify_booking_changed(&entry).await {
            eprintln!("error sending booking notifications: {err:#}");
        }
    });
}

async fn oob_calendars(repo: &dyn Repository, locale: Locale) -> Result<Markup, AppError> {
    let now = jiff::Zoned::now();
    Ok(views::calendars(&views::CalendarsOpts {
        id: Some(CALENDARS_ELEMENT_ID.to_string()),
        locale,
        start_year: now.year(),
        start_month: now.month(),
        sorted_bookings: &list_bookings(repo, &now)
            .await
            .context("listing bookings")?,
        max_capacity: 6,
        people: &list_people(repo).await.context("listing people")?,
        hx_swap_oob: true,
    }))
}

struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("request failed: {:#}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Locale::default().strings().internal_error,
        )
            .into_response()
    }
}
