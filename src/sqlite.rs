use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use tokio_rusqlite::{Connection, rusqlite};

use crate::bookings::{
    Booking, BookingId, BookingInput, Person, PersonId, Repository, SaveBookingError,
};

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

    async fn save_booking(
        &self,
        id: Option<BookingId>,
        booking: &BookingInput,
    ) -> Result<Booking, SaveBookingError> {
        let booking = booking.clone();
        self.conn
            .call(move |conn| -> rusqlite::Result<Option<Booking>> {
                match id {
                    None => {
                        let new_id: i64 = conn.query_row(
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
                        Ok(Some(persisted(BookingId::new(parse_u32(new_id)?), booking)))
                    }
                    Some(id) => {
                        let affected = conn.execute(
                            "UPDATE bookings \
                             SET start_date = ?1, end_date = ?2, creator_id = ?3, guest_count = ?4 \
                             WHERE id = ?5",
                            rusqlite::params![
                                format_date(&booking.start_date()),
                                format_date(&booking.end_date()),
                                u32::from(booking.creator_id()),
                                booking.guest_count(),
                                u32::from(id),
                            ],
                        )?;
                        Ok((affected > 0).then(|| persisted(id, booking)))
                    }
                }
            })
            .await
            .map_err(|e| SaveBookingError::Internal(e.into()))?
            .ok_or(SaveBookingError::NotFound)
    }

    async fn list_bookings(&self, after: jiff::civil::Date) -> anyhow::Result<Vec<Booking>> {
        let after = format_date(&after);
        self.conn
            .call(move |conn| -> rusqlite::Result<Vec<Booking>> {
                let mut stmt = conn.prepare(
                    "SELECT id, start_date, end_date, creator_id, guest_count \
                     FROM bookings WHERE end_date >= ?1 ORDER BY start_date, id",
                )?;
                let bookings = stmt
                    .query_map(rusqlite::params![after], |row| {
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

    async fn delete_booking(&self, id: BookingId) -> anyhow::Result<()> {
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "DELETE FROM bookings WHERE id = ?1",
                    rusqlite::params![u32::from(id)],
                )?;
                Ok(())
            })
            .await
            .context("deleting booking")
    }
}

fn format_date(d: &jiff::civil::Date) -> String {
    d.strftime("%Y%m%d").to_string()
}

fn parse_date(s: &str) -> rusqlite::Result<jiff::civil::Date> {
    jiff::civil::Date::strptime("%Y%m%d", s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
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

    #[tokio::test]
    async fn creates_then_updates_booking() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        assert_eq!(person.name, "Alice");

        let created = repo
            .save_booking(
                None,
                &BookingInput::new(start_date(), end_date(), person.id, 3).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.guest_count, 3);

        let updated = repo
            .save_booking(
                Some(created.id),
                &BookingInput::new(start_date(), end_date(), person.id, 5).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.guest_count, 5);
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
    async fn list_bookings_filters_and_orders() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();

        let past = repo
            .save_booking(
                None,
                &BookingInput::new(date(2026, 1, 10), date(2026, 1, 12), person.id, 3).unwrap(),
            )
            .await
            .unwrap();
        let later = repo
            .save_booking(
                None,
                &BookingInput::new(date(2026, 8, 5), date(2026, 8, 7), person.id, 3).unwrap(),
            )
            .await
            .unwrap();
        let ongoing = repo
            .save_booking(
                None,
                &BookingInput::new(date(2026, 7, 20), date(2026, 7, 28), person.id, 3).unwrap(),
            )
            .await
            .unwrap();

        let bookings = repo.list_bookings(date(2026, 7, 27)).await.unwrap();
        let ids: Vec<_> = bookings.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![ongoing.id, later.id]);
        assert!(!ids.contains(&past.id));
        assert_eq!(bookings[0].start_date, date(2026, 7, 20));
        assert_eq!(bookings[0].end_date, date(2026, 7, 28));
    }

    #[tokio::test]
    async fn deletes_booking_and_ignores_missing_ones() {
        let repo = setup().await;
        let person = repo.save_person("Alice").await.unwrap();
        let created = repo
            .save_booking(
                None,
                &BookingInput::new(start_date(), end_date(), person.id, 3).unwrap(),
            )
            .await
            .unwrap();

        repo.delete_booking(created.id).await.unwrap();
        assert!(repo.list_bookings(start_date()).await.unwrap().is_empty());

        repo.delete_booking(created.id).await.unwrap();
    }

    #[tokio::test]
    async fn updating_missing_booking_is_not_found() {
        let repo = setup().await;
        let person = repo.save_person("Bob").await.unwrap();
        let booking = BookingInput::new(start_date(), end_date(), person.id, 3).unwrap();
        assert!(matches!(
            repo.save_booking(Some(BookingId::new(999)), &booking).await,
            Err(SaveBookingError::NotFound)
        ));
    }
}
