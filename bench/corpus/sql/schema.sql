-- Schema: tables, constraints, indexes, views and a function.
CREATE TABLE customers (
    id          BIGINT PRIMARY KEY,
    name        VARCHAR(200) NOT NULL,
    email       VARCHAR(320) UNIQUE,
    created_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id) ON DELETE CASCADE,
    status      VARCHAR(16) NOT NULL CHECK (status IN ('pending', 'done', 'failed')),
    total       NUMERIC(12, 2) NOT NULL DEFAULT 0,
    note        TEXT
);

CREATE INDEX orders_by_customer ON orders (customer_id, status);

CREATE VIEW customer_totals AS
SELECT c.id, c.name, COALESCE(SUM(o.total), 0) AS spent, COUNT(o.id) AS orders
FROM customers c
LEFT JOIN orders o ON o.customer_id = c.id AND o.status <> 'failed'
GROUP BY c.id, c.name;

INSERT INTO customers (id, name, email) VALUES
    (1, 'Zoë O''Brien', 'zoe@example.invalid'),
    (2, 'Ångström Ltd', NULL);
