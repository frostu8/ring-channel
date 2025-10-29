-- SQL script for finding all matchups a player has played in a period.
-- Inputs:
--   $1: id of player
--   $2: time from
--   $3: time to
-- Outputs: opponent rating r.*, b.status, posiiton, mw.finish_time

WITH recent_ratings AS (
    SELECT r1.*
    FROM
        player p, rating r1, rating r2
    WHERE
        p.id = r1.player_id
        AND p.id = r2.player_id
    GROUP BY p.id, r1.inserted_at
    HAVING r1.inserted_at = MAX(r2.inserted_at)
)
SELECT
    r.*,
    b.status,
    -- +1 to correct for self
    COUNT(*) + 1 - COUNT(NOT op.no_contest AND me.finish_time < op.finish_time) AS position,
    IIF(MIN(op.finish_time) IS NOT NULL, MIN(op.finish_time), me.finish_time) AS finish_time,
    me.no_contest
FROM
    battle b, participant op, participant me, recent_ratings r
WHERE
    me.match_id = b.id
    AND op.match_id = b.id
    AND op.player_id = r.player_id
    -- Filter out opponents and "me"
    AND me.player_id = $1
    AND NOT op.player_id = $1
    -- Only get matches between the bounds
    AND b.concluded_at >= $2
    AND b.concluded_at < $3
-- Group by battles to count how many we are ahead
GROUP BY b.id, b.status, b.inserted_at, me.finish_time, me.no_contest
-- we only want matches where two players participated
HAVING COUNT(*) = 1
ORDER BY b.inserted_at ASC
