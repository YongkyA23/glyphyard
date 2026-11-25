-- Copies of one command queued to a merged group share the id of the first copy; NULL = sent to one install.
ALTER TABLE commands ADD COLUMN fanout INTEGER;
CREATE INDEX commands_fanout ON commands(fanout);
