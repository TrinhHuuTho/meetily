-- Language code detected for each transcript segment (e.g. 'en', 'vi', 'zh').
-- NULL for segments transcribed before language tracking or by engines without detection.
ALTER TABLE transcripts ADD COLUMN language TEXT;
