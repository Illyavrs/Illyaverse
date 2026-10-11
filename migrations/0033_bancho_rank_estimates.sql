-- Optional "compare me with Bancho" display rank. A player who opts in (preferences.bancho_rank)
-- sees the rank their current pp would hold among official Bancho players. The table holds
-- approximate (rank, pp) anchors per ruleset; operators may refine them with plain UPDATE/INSERT.
CREATE TABLE bancho_rank_anchors (
    mode SMALLINT NOT NULL CHECK (mode BETWEEN 0 AND 3),
    rank BIGINT NOT NULL CHECK (rank > 0),
    pp DOUBLE PRECISION NOT NULL CHECK (pp >= 0),
    PRIMARY KEY (mode, rank),
    UNIQUE (mode, pp)
);

INSERT INTO bancho_rank_anchors(mode, rank, pp) VALUES
 (0,1,17500),(0,10,15500),(0,100,13200),(0,1000,10800),(0,5000,8800),(0,10000,7900),(0,50000,5900),
 (0,100000,4900),(0,250000,3400),(0,500000,2300),(0,1000000,1300),(0,2000000,550),(0,3000000,120),(0,4000000,1),
 (1,1,16000),(1,100,12500),(1,1000,9500),(1,10000,6300),(1,50000,4000),(1,100000,3000),(1,250000,1800),
 (1,500000,900),(1,1000000,250),(1,1500000,1),
 (2,1,15000),(2,100,11500),(2,1000,8800),(2,10000,5600),(2,50000,3300),(2,100000,2400),(2,250000,1300),
 (2,500000,550),(2,800000,1),
 (3,1,22000),(3,100,17500),(3,1000,13500),(3,10000,8800),(3,50000,5700),(3,100000,4300),(3,250000,2600),
 (3,500000,1400),(3,1000000,400),(3,1500000,1);

-- Log-linear interpolation between the two anchors that surround the given pp.
CREATE FUNCTION bancho_rank_estimate(ruleset SMALLINT, player_pp DOUBLE PRECISION) RETURNS BIGINT
LANGUAGE plpgsql STABLE AS $$
DECLARE
    better RECORD;
    worse RECORD;
    t DOUBLE PRECISION;
BEGIN
    SELECT rank, pp INTO better FROM bancho_rank_anchors
      WHERE mode = ruleset AND pp >= player_pp ORDER BY pp ASC LIMIT 1;
    IF NOT FOUND THEN
        RETURN 1;
    END IF;
    SELECT rank, pp INTO worse FROM bancho_rank_anchors
      WHERE mode = ruleset AND pp < player_pp ORDER BY pp DESC LIMIT 1;
    IF NOT FOUND THEN
        RETURN better.rank;
    END IF;
    t := (better.pp - player_pp) / (better.pp - worse.pp);
    RETURN GREATEST(1, round(exp(ln(better.rank) + t * (ln(worse.rank) - ln(better.rank)))))::bigint;
END $$;
