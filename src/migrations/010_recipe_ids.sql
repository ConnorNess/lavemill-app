CREATE TABLE recipe_id_sequence (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_id INTEGER NOT NULL CHECK (last_id >= 0)
);

INSERT INTO recipe_id_sequence (singleton, last_id)
SELECT 1, max(0, coalesce(max(id), 0)) FROM recipes;

CREATE TRIGGER track_recipe_id AFTER INSERT ON recipes
WHEN NEW.id > (SELECT last_id FROM recipe_id_sequence WHERE singleton = 1)
BEGIN
    UPDATE recipe_id_sequence SET last_id = NEW.id WHERE singleton = 1;
END;
