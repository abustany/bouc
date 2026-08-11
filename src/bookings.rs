use anyhow::Result;
use jiff::civil::Date;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone)]
pub struct Booking {
    pub id: BookingId,
    pub start_date: Date,
    pub end_date: Date,
    pub creator_id: PersonId,
    pub guest_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[async_trait::async_trait]
pub trait Repository: Send + Sync {
    /// Persist a person identified by their name.
    async fn save_person(&self, name: &str) -> Result<Person>;

    /// List every person, ordered by name.
    async fn list_people(&self) -> Result<Vec<Person>>;

    /// Create a new booking (`id` is `None`) or update an existing one (`id`
    /// is `Some`). Returns the persisted booking.
    async fn save_booking(
        &self,
        id: Option<BookingId>,
        booking: &BookingInput,
    ) -> Result<Booking, SaveBookingError>;

    async fn list_bookings(&self, after: Date) -> Result<Vec<Booking>>;

    /// Delete a booking, doing nothing if no booking has that id.
    async fn delete_booking(&self, id: BookingId, creator_id: PersonId) -> Result<()>;
}

#[derive(Debug, Error)]
pub enum SaveBookingError {
    /// No booking exists with the id targeted by an update.
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
