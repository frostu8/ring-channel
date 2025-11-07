-- Allow players to claim certain characters
ALTER TABLE participant ADD COLUMN skin VARCHAR(255);
-- Log their kartspeed and kartweight seperately for now, just in case we get
-- restat.lua
ALTER TABLE participant ADD COLUMN kart_speed INTEGER;
ALTER TABLE participant ADD COLUMN kart_weight INTEGER;
