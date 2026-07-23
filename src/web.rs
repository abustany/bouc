use std::sync::Arc;
use std::{collections::HashMap, convert::Infallible};

use anyhow::Context;
use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use maud::Markup;
use rust_embed::RustEmbed;
use serde::Deserialize;

use crate::bookings::{self, BookingId, BookingInput};
use crate::bookings::{Repository, validate_person_name};
use crate::strings::Locale;
use crate::views::{self, CALENDARS_ELEMENT_ID};

type SharedRepository = Arc<dyn Repository>;

const HX_TRIGGER: &str = "hx-trigger";

/// Client-side event fired once a booking has been persisted, see
/// `src/day-popover.ts`.
const BOOKING_SAVED_EVENT: &str = "booking-saved";

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub fn router(repo: SharedRepository) -> Router {
    Router::new()
        // start of app routes
        .route("/", get(index))
        .route("/bookings", post(create_booking))
        .route("/bookings/{id}", delete(delete_booking))
        // end of app routes, any route *after* the route_layer call below does
        // not get the Vary header properly set to accept-language.
        .route_layer(middleware::from_fn(set_vary_accept_language))
        .route("/assets/{*path}", get(serve_asset))
        .with_state(repo)
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

async fn list_bookings(
    repo: &SharedRepository,
    now: &jiff::Zoned,
) -> anyhow::Result<Vec<views::Booking>> {
    Ok(repo
        .list_bookings(jiff::civil::date(now.year(), now.month(), 1))
        .await
        .context("listing bookings")?
        .iter()
        .map(
            |bookings::Booking {
                 id,
                 start_date,
                 end_date,
                 guest_count,
                 creator_id,
                 ..
             }| {
                views::Booking {
                    id: *id,
                    start_date: *start_date,
                    end_date: *end_date,
                    guest_count: *guest_count,
                    creator_id: *creator_id,
                }
            },
        )
        .collect::<Vec<_>>())
}

async fn list_people(
    repo: &SharedRepository,
) -> anyhow::Result<HashMap<bookings::PersonId, bookings::Person>> {
    Ok(repo
        .list_people()
        .await
        .context("listing people")?
        .into_iter()
        .map(|p| (p.id, p))
        .collect::<HashMap<_, _>>())
}

async fn index(State(repo): State<SharedRepository>, locale: Locale) -> Result<Markup, AppError> {
    let now = jiff::Zoned::now();
    Ok(views::index(views::IndexOpts {
        locale,
        start_year: now.year(),
        start_month: now.month(),
        sorted_bookings: &list_bookings(&repo, &now)
            .await
            .context("listing bookings")?,
        max_capacity: 6,
        people: &list_people(&repo).await.context("listing people")?,
    }))
}

#[derive(Deserialize)]
struct CreateBookingForm {
    id: Option<String>,
    name: String,
    start_date: String,
    end_date: String,
    guest_count: String,
}

async fn create_booking(
    State(repo): State<SharedRepository>,
    headers: HeaderMap,
    locale: Locale,
    Form(form): Form<CreateBookingForm>,
) -> Result<Response, AppError> {
    let is_htmx = headers.contains_key("hx-request");

    let Ok(start_date) = form.start_date.parse::<jiff::civil::Date>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid start date").into_response());
    };

    let Ok(end_date) = form.end_date.parse::<jiff::civil::Date>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid end date").into_response());
    };

    let Ok(guest_count) = form.guest_count.parse::<u32>() else {
        return Ok((StatusCode::BAD_REQUEST, "invalid guest count").into_response());
    };

    let Ok(name) = validate_person_name(&form.name) else {
        return Ok((StatusCode::BAD_REQUEST, "invalid name").into_response());
    };
    let creator = repo.save_person(&name).await.context("saving creator")?;

    let Ok(booking) = BookingInput::new(start_date, end_date, creator.id, guest_count) else {
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

    repo.save_booking(booking_id, &booking)
        .await
        .context("saving booking")?;

    if is_htmx {
        let calendars = oob_calendars(&repo, locale).await?;
        Ok(([(HX_TRIGGER, BOOKING_SAVED_EVENT)], calendars).into_response())
    } else {
        Ok(Redirect::to("/").into_response())
    }
}

async fn delete_booking(
    State(repo): State<SharedRepository>,
    locale: Locale,
    Path(id): Path<u32>,
) -> Result<Markup, AppError> {
    repo.delete_booking(BookingId::new(id))
        .await
        .context("deleting booking")?;

    oob_calendars(&repo, locale).await
}

async fn oob_calendars(repo: &SharedRepository, locale: Locale) -> Result<Markup, AppError> {
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
