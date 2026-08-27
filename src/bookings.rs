use anyhow::Result;
use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersonId(u32);

impl From<PersonId> for u32 {
    fn from(id: PersonId) -> Self {
        id.0
    }
}

impl PersonId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Booking {
    pub id: BookingId,
    pub start_date: Date,
    pub end_date: Date,
    pub creator_id: PersonId,
    pub guest_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BookingId(u32);

impl From<BookingId> for u32 {
    fn from(id: BookingId) -> Self {
        id.0
    }
}

impl BookingId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone)]
pub struct BookingInput {
    start_date: Date,
    end_date: Date,
    creator_id: PersonId,
    guest_count: u32,
}

impl BookingInput {
    pub fn new(
        start_date: Date,
        end_date: Date,
        creator: PersonId,
        guest_count: u32,
    ) -> Result<Self, BookingError> {
        let b = Self {
            start_date,
            end_date,
            creator_id: creator,
            guest_count,
        };
        validate_booking(&b)?;
        Ok(b)
    }

    pub fn start_date(&self) -> Date {
        self.start_date
    }

    pub fn end_date(&self) -> Date {
        self.end_date
    }

    pub fn creator_id(&self) -> PersonId {
        self.creator_id
    }

    pub fn guest_count(&self) -> u32 {
        self.guest_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BookingLogEntryId(u32);

impl From<BookingLogEntryId> for u32 {
    fn from(id: BookingLogEntryId) -> Self {
        id.0
    }
}

impl BookingLogEntryId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone)]
pub struct BookingLogEntry {
    pub id: BookingLogEntryId,
    pub creator_id: PersonId,
    pub create_time: Timestamp,
    pub payload: BookingLogEntryPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BookingLogEntryPayload {
    BookingCreated { booking: Booking },
    BookingChanged { before: Booking, after: Booking },
    BookingDeleted { booking: Booking },
}

#[derive(Debug, Clone)]
pub struct NotificationSubscription {
    pub person_id: PersonId,
    pub payload: NotificationSubscriptionPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationSubscriptionPayload {
    Pending {
        email: String,
        verification_token: String,
    }, // user asked to be notified, needs to confirm email address
    Active {
        email: String,
        locale: String,
        unsubscribe_token: String,
    }, // user is receiving notifications at the given address
    Disabled, // user declined notifications
}

pub enum ListBookingsFilter {
    EndsAfter(Date),
    IntersectsRange { start: Date, end: Date },
}

#[async_trait::async_trait]
pub trait Repository: Send + Sync {
    /// Persist a person identified by their name.
    async fn save_person(&self, name: &str) -> Result<Person>;

    /// List every person, ordered by name.
    async fn list_people(&self) -> Result<Vec<Person>>;

    /// Gets the notification subscription for a given person.
    async fn get_notification_subscription(
        &self,
        person_id: PersonId,
    ) -> Result<Option<NotificationSubscription>>;

    /// Creates or updates the notification subscription for a given person.
    async fn save_notification_subscription(
        &self,
        subscription: &NotificationSubscription,
    ) -> Result<()>;

    /// Saves a new booking + a log entry.
    async fn create_booking(&self, booking: &BookingInput) -> Result<(Booking, BookingLogEntry)>;

    /// Updates an existing booking + adds a log entry.
    async fn update_booking(
        &self,
        id: BookingId,
        creator_id: PersonId,
        booking: &BookingInput,
    ) -> Result<(Booking, BookingLogEntry), UpdateBookingError>;

    /// List all bookings after a given date.
    async fn list_bookings(&self, filter: ListBookingsFilter) -> Result<Vec<Booking>>;

    /// Delete a booking.
    async fn delete_booking(
        &self,
        id: BookingId,
        creator_id: PersonId,
    ) -> Result<BookingLogEntry, DeleteBookingError>;

    /// Load one page of booking log entries, newest first. Pass the id of the
    /// last entry of the previous page as `before` to get the next one.
    async fn list_booking_log(
        &self,
        before: Option<BookingLogEntryId>,
    ) -> Result<Vec<BookingLogEntry>>;
}

#[derive(Debug, Error)]
pub enum UpdateBookingError {
    /// No booking has that id, or it was created by somebody else.
    #[error("booking not found")]
    NotFound,
    /// The underlying storage failed.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum DeleteBookingError {
    /// No booking has that id, or it was created by somebody else.
    #[error("booking not found")]
    NotFound,
    /// The underlying storage failed.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PersonNameError {
    #[error("person name is empty")]
    Empty,
}

/// Validate a person's name, returning the trimmed value on success.
pub fn validate_person_name(name: &str) -> Result<String, PersonNameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PersonNameError::Empty);
    }
    Ok(name.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BookingError {
    #[error("end date is before start date")]
    EndBeforeStart,
    #[error("guest count is zero")]
    GuestCountZero,
}

/// Validate a booking's fields (end not before start, at least one guest).
pub fn validate_booking(booking: &BookingInput) -> Result<(), BookingError> {
    if booking.end_date < booking.start_date {
        return Err(BookingError::EndBeforeStart);
    }

    if booking.guest_count == 0 {
        return Err(BookingError::GuestCountZero);
    }

    Ok(())
}
