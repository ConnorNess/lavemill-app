CREATE TABLE recipes (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);

CREATE TABLE recipe_ingredient_sections (
    id INTEGER PRIMARY KEY,
    recipe_id INTEGER NOT NULL REFERENCES recipes (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    heading TEXT,
    UNIQUE (recipe_id, position)
);

CREATE TABLE recipe_ingredients (
    section_id INTEGER NOT NULL REFERENCES recipe_ingredient_sections (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    quantity TEXT,
    PRIMARY KEY (section_id, position)
);

CREATE TABLE recipe_step_sections (
    id INTEGER PRIMARY KEY,
    recipe_id INTEGER NOT NULL REFERENCES recipes (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    heading TEXT,
    UNIQUE (recipe_id, position)
);

CREATE TABLE recipe_steps (
    section_id INTEGER NOT NULL REFERENCES recipe_step_sections (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    instructions TEXT NOT NULL CHECK (length(trim(instructions)) > 0),
    note TEXT,
    PRIMARY KEY (section_id, position)
);
