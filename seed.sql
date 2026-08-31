INSERT INTO people (id, name, name_key) VALUES (1, 'Alice', 'alice') ON CONFLICT DO NOTHING;
INSERT INTO people (id, name, name_key) VALUES (2, 'Bob', 'bob') ON CONFLICT DO NOTHING;
INSERT INTO bookings (start_date, end_date, creator_id, guest_count) VALUES ('20260825', '20260830', 1, 3);
INSERT INTO bookings (start_date, end_date, creator_id, guest_count) VALUES ('20260829', '20260902', 2, 3);
