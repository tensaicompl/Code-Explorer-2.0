WITH recent AS (
    SELECT o.*, ROW_NUMBER() OVER (PARTITION BY customer_id ORDER BY id DESC) AS rn
    FROM orders o
    WHERE o.status = 'done'
),
ranked AS (
    SELECT customer_id, total, rn,
           SUM(total) OVER (PARTITION BY customer_id) AS customer_total
    FROM recent
    WHERE rn <= 3
)
SELECT c.name,
       r.customer_total,
       CASE
           WHEN r.customer_total > 1000 THEN 'gold'
           WHEN r.customer_total > 100 THEN 'silver'
           ELSE 'bronze'
       END AS tier
FROM ranked r
JOIN customers c ON c.id = r.customer_id
WHERE EXISTS (SELECT 1 FROM orders x WHERE x.customer_id = c.id AND x.total > 0)
ORDER BY r.customer_total DESC, c.name
LIMIT 10;

UPDATE orders SET status = 'failed' WHERE total < 0 AND note IS NULL;
DELETE FROM orders WHERE id IN (SELECT id FROM orders WHERE status = 'failed');
