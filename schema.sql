CREATE TABLE people (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE
) strict;

CREATE TABLE notification_subscriptions (
  person_id INTEGER PRIMARY KEY REFERENCES people(id),
  payload TEXT NOT NULL -- JSON
) strict;

CREATE TABLE bookings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  start_date TEXT NOT NULL, -- YYYYMMDD
  end_date TEXT NOT NULL, -- YYYYMMDD
  creator_id INTEGER NOT NULL REFERENCES people(id),
  guest_count INTEGER NOT NULL
) strict;

CREATE TABLE bookings_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  creator_id INTEGER NOT NULL REFERENCES people(id),
  create_time TEXT NOT NULL, -- ISO8601
  payload TEXT NOT NULL -- JSON
) strict;
