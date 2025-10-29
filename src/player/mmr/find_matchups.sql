-- SQL script for finding all matchups a player has played in a period.
-- Inputs:
--   $1: id of player
--   $2: time from
--   $3: time to
-- Outputs: opponent rating r.*, win

SELECT
    r.*,
    b.status,
    -- +1 to correct for self
    COUNT(*) + 1 - COUNT(NOT op.no_contest AND me.finish_time < op.finish_time) AS position,
    MIN(op.finish_time) AS finish_time,
    me.no_contest
FROM
    battle b, participant op, participant me, rating r
WHERE
    me.match_id = b.id
    AND op.match_id = b.id
    -- Filter out opponents and "me"
    AND me.player_id = $1
    AND NOT op.player_id = $1
    -- Only get matches between the bounds
    AND b.concluded_at >= $2
    AND b.concluded_at < $3
    -- Get the opponent's rating
    AND r.id = (
        SELECT id
        FROM rating ri
        WHERE ri.player_id = op.player_id
        ORDER BY ri.inserted_at DESC
    )
-- Group by battles to count how many we are ahead
GROUP BY b.id, b.status, b.inserted_at, me.no_contest
-- we only want matches where two players participated
HAVING COUNT(*) = 1
ORDER BY b.inserted_at ASC
