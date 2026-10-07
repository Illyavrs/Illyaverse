ALTER TABLE scores ADD COLUMN api_mods jsonb;
ALTER TABLE scores ADD COLUMN pp_eligible boolean NOT NULL DEFAULT true;
UPDATE scores SET pp_eligible=false WHERE (mods & 10368)<>0;
ALTER TABLE scores ADD CONSTRAINT scores_api_mods_array CHECK (api_mods IS NULL OR (jsonb_typeof(api_mods)='array' AND jsonb_array_length(api_mods)<=32));
