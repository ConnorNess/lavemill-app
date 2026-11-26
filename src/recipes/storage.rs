use super::{
    AddRecipeIngredients, Ingredient, IngredientPosition, Recipe, RecipeCard, RecipeContents,
    SaveRecipe, Section, Step,
};
use rusqlite::{params, Connection, OptionalExtension, Result, Row, TransactionBehavior};

pub(super) fn list(db: &Connection) -> Result<Vec<RecipeCard>> {
    let mut query =
        db.prepare("SELECT id, title FROM recipes ORDER BY title COLLATE NOCASE, id")?;
    let cards = query.query_map([], |row| {
        Ok(RecipeCard {
            id: row.get(0)?,
            title: row.get(1)?,
        })
    })?;
    cards.collect()
}

pub(super) fn get(db: &mut Connection, id: i64) -> Result<Option<Recipe>> {
    let tx = db.transaction()?;
    let header = tx
        .query_row(
            "SELECT title, revision FROM recipes WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((title, revision)) = header else {
        return Ok(None);
    };
    let ingredients = read_ingredients(&tx, id)?;
    let steps = read_sections(
        &tx,
        id,
        "SELECT id, heading FROM recipe_step_sections WHERE recipe_id = ?1 ORDER BY position",
        "SELECT instructions, note FROM recipe_steps WHERE section_id = ?1 ORDER BY position",
        |row| {
            Ok(Step {
                instructions: row.get(0)?,
                note: row.get(1)?,
            })
        },
    )?;
    tx.commit()?;
    Ok(Some(Recipe {
        revision,
        contents: RecipeContents {
            title,
            ingredients,
            steps,
        },
    }))
}

pub(super) fn add_to_shopping(
    db: &mut Connection,
    actor: i64,
    id: i64,
    revision: i64,
    mut positions: Vec<IngredientPosition>,
) -> Result<AddRecipeIngredients> {
    if positions.is_empty() {
        return Ok(AddRecipeIngredients::InvalidSelection);
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current_revision = tx
        .query_row("SELECT revision FROM recipes WHERE id = ?1", [id], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?;
    if current_revision != Some(revision) {
        return Ok(AddRecipeIngredients::StaleRecipe);
    }

    let sections = read_ingredients(&tx, id)?;
    positions.sort_unstable();
    positions.dedup();
    let selected: Option<Vec<_>> = positions
        .iter()
        .map(|&(section, entry)| sections.get(section)?.entries.get(entry))
        .collect();
    let Some(selected) = selected else {
        return Ok(AddRecipeIngredients::InvalidSelection);
    };
    let count = selected.len();
    let added = crate::shopping_storage::add_missing(
        &tx,
        actor,
        selected
            .into_iter()
            .map(|ingredient| (ingredient.name.as_str(), ingredient.quantity.as_deref())),
    )?;
    tx.commit()?;
    Ok(AddRecipeIngredients::Added {
        added,
        skipped: count - added,
    })
}

fn read_ingredients(db: &Connection, id: i64) -> Result<Vec<Section<Ingredient>>> {
    read_sections(
        db,
        id,
        "SELECT id, heading FROM recipe_ingredient_sections WHERE recipe_id = ?1 ORDER BY position",
        "SELECT name, quantity FROM recipe_ingredients WHERE section_id = ?1 ORDER BY position",
        |row| {
            Ok(Ingredient {
                name: row.get(0)?,
                quantity: row.get(1)?,
            })
        },
    )
}

fn read_sections<T>(
    db: &Connection,
    recipe_id: i64,
    section_sql: &str,
    entry_sql: &str,
    map_entry: fn(&Row<'_>) -> Result<T>,
) -> Result<Vec<Section<T>>> {
    let mut section_query = db.prepare(section_sql)?;
    let mut entry_query = db.prepare(entry_sql)?;
    let sections = section_query.query_map([recipe_id], |row| {
        let id: i64 = row.get(0)?;
        let entries = entry_query
            .query_map([id], map_entry)?
            .collect::<Result<_>>()?;
        Ok(Section {
            heading: row.get(1)?,
            entries,
        })
    })?;
    sections.collect()
}

pub(super) fn validate(
    mut contents: RecipeContents,
) -> std::result::Result<RecipeContents, &'static str> {
    fn trim(text: &mut String) {
        *text = text.trim().to_owned();
    }

    fn optional(text: &mut Option<String>) {
        *text = text
            .take()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
    }

    trim(&mut contents.title);
    if contents.title.is_empty() {
        return Err("Enter a recipe title.");
    }
    if contents.ingredients.is_empty() || contents.steps.is_empty() {
        return Err("Add at least one ingredient and one step.");
    }
    for section in &mut contents.ingredients {
        optional(&mut section.heading);
        if section.entries.is_empty() {
            return Err(
                "Add an ingredient to each ingredient section or remove the empty section.",
            );
        }
        for ingredient in &mut section.entries {
            trim(&mut ingredient.name);
            optional(&mut ingredient.quantity);
            if ingredient.name.is_empty() {
                return Err("Enter a name for each ingredient.");
            }
        }
    }
    for section in &mut contents.steps {
        optional(&mut section.heading);
        if section.entries.is_empty() {
            return Err("Add a step to each step section or remove the empty section.");
        }
        for step in &mut section.entries {
            trim(&mut step.instructions);
            optional(&mut step.note);
            if step.instructions.is_empty() {
                return Err("Enter instructions for each step.");
            }
        }
    }
    Ok(contents)
}

pub(super) fn create(db: &mut Connection, contents: &RecipeContents) -> Result<SaveRecipe> {
    let tx = db.transaction()?;
    let id = tx.query_row(
        "INSERT INTO recipes (title) VALUES (?1) RETURNING id",
        [&contents.title],
        |row| row.get(0),
    )?;
    write_contents(&tx, id, contents)?;
    tx.commit()?;
    Ok(SaveRecipe::Saved(id))
}

pub(super) fn update(
    db: &mut Connection,
    id: i64,
    revision: i64,
    contents: &RecipeContents,
) -> Result<SaveRecipe> {
    let tx = db.transaction()?;
    let changed = tx.execute(
        "UPDATE recipes SET title = ?3, revision = revision + 1 WHERE id = ?1 AND revision = ?2",
        params![id, revision, contents.title],
    )?;
    if changed == 0 {
        let exists = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM recipes WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        return Ok(if exists {
            SaveRecipe::Conflict
        } else {
            SaveRecipe::Invalid("Recipe not found.".into())
        });
    }
    tx.execute(
        "DELETE FROM recipe_ingredient_sections WHERE recipe_id = ?1",
        [id],
    )?;
    tx.execute(
        "DELETE FROM recipe_step_sections WHERE recipe_id = ?1",
        [id],
    )?;
    write_contents(&tx, id, contents)?;
    tx.commit()?;
    Ok(SaveRecipe::Saved(id))
}

fn write_contents(db: &Connection, id: i64, contents: &RecipeContents) -> Result<()> {
    write_sections(
        db, id, &contents.ingredients,
        "INSERT INTO recipe_ingredient_sections (recipe_id, position, heading) VALUES (?1, ?2, ?3) RETURNING id",
        "INSERT INTO recipe_ingredients (section_id, position, name, quantity) VALUES (?1, ?2, ?3, ?4)",
        |entry| (&entry.name, entry.quantity.as_deref()),
    )?;
    write_sections(
        db, id, &contents.steps,
        "INSERT INTO recipe_step_sections (recipe_id, position, heading) VALUES (?1, ?2, ?3) RETURNING id",
        "INSERT INTO recipe_steps (section_id, position, instructions, note) VALUES (?1, ?2, ?3, ?4)",
        |entry| (&entry.instructions, entry.note.as_deref()),
    )
}

fn write_sections<T>(
    db: &Connection,
    recipe_id: i64,
    sections: &[Section<T>],
    section_sql: &str,
    entry_sql: &str,
    fields: fn(&T) -> (&str, Option<&str>),
) -> Result<()> {
    let mut section_query = db.prepare(section_sql)?;
    let mut entry_query = db.prepare(entry_sql)?;
    for (position, section) in sections.iter().enumerate() {
        let id: i64 = section_query
            .query_row(params![recipe_id, position, section.heading], |row| {
                row.get(0)
            })?;
        for (position, entry) in section.entries.iter().enumerate() {
            let (text, detail) = fields(entry);
            entry_query.execute(params![id, position, text, detail])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shopping_fixture(db: &Connection) {
        db.execute_batch(
            "INSERT INTO users (id, username, first_name, last_name, password_hash)
             VALUES (1, 'member', 'Household', 'Member', 'unused');
             INSERT INTO recipes (id, title) VALUES (1, 'Toast');
             INSERT INTO recipe_ingredient_sections VALUES (1, 1, 0, NULL), (2, 1, 1, 'Topping');
             INSERT INTO recipe_ingredients VALUES
                 (1, 0, 'Bread', '1 slice'), (1, 1, 'Butter', NULL), (1, 2, 'Oil', NULL),
                 (2, 0, 'butter', NULL), (2, 1, 'Jam', NULL), (2, 2, 'Honey', '1 tsp');
             INSERT INTO shopping_items (name, quantity, created_by, completed_at, removed_at, removed_by)
             VALUES ('BREAD', '1 slice', 1, NULL, NULL, NULL),
                    ('Butter', '1 pack', 1, NULL, NULL, NULL),
                    ('Butter', NULL, 1, unixepoch(), NULL, NULL),
                    ('Jam', NULL, 1, NULL, unixepoch(), 1);",
        )
        .unwrap();
    }

    #[test]
    fn imports_selected_ingredients_in_order_and_skips_only_active_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipes.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        shopping_fixture(&db);
        let selection = vec![(1, 2), (1, 1), (1, 0), (0, 1), (0, 0), (0, 1)];
        assert_eq!(
            add_to_shopping(&mut db, 1, 1, 1, selection.clone()).unwrap(),
            AddRecipeIngredients::Added {
                added: 3,
                skipped: 2
            }
        );
        assert_eq!(
            add_to_shopping(&mut db, 1, 1, 1, selection).unwrap(),
            AddRecipeIngredients::Added {
                added: 0,
                skipped: 5
            }
        );
        let mut rows = db
            .prepare(
                "SELECT name, quantity, created_by, completed_at IS NULL
                 FROM shopping_items WHERE id > 4 ORDER BY id",
            )
            .unwrap();
        let items = rows
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            items,
            [
                ("Butter".into(), None, 1, true),
                ("Jam".into(), None, 1, true),
                ("Honey".into(), Some("1 tsp".into()), 1, true),
            ]
        );
    }

    #[test]
    fn rejects_invalid_or_stale_selection_and_rolls_back_failed_inserts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipes.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        shopping_fixture(&db);
        for positions in [vec![], vec![(0, 1), (0, 3)], vec![(2, 0)]] {
            assert_eq!(
                add_to_shopping(&mut db, 1, 1, 1, positions).unwrap(),
                AddRecipeIngredients::InvalidSelection
            );
        }
        for (id, revision) in [(1, 2), (2, 1)] {
            assert_eq!(
                add_to_shopping(&mut db, 1, id, revision, vec![(0, 1)]).unwrap(),
                AddRecipeIngredients::StaleRecipe
            );
        }
        db.execute_batch(
            "CREATE TRIGGER reject_jam BEFORE INSERT ON shopping_items WHEN NEW.name = 'Jam'
             BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
        )
        .unwrap();
        assert!(add_to_shopping(&mut db, 1, 1, 1, vec![(0, 1), (1, 1)]).is_err());
        let count: i64 = db
            .query_row("SELECT count(*) FROM shopping_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }

    #[test]
    fn editing_replaces_sections_without_overwriting_a_newer_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipes.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut first = crate::database::open(&path).unwrap();
        let mut second = crate::database::open(&path).unwrap();
        let mut draft = RecipeContents {
            title: "Toast".into(),
            ingredients: vec![
                Section {
                    heading: None,
                    entries: vec![Ingredient {
                        name: "Bread".into(),
                        quantity: Some("a slice".into()),
                    }],
                },
                Section {
                    heading: Some("Topping".into()),
                    entries: vec![Ingredient {
                        name: "Jam".into(),
                        quantity: None,
                    }],
                },
            ],
            steps: vec![Section {
                heading: Some("Grill".into()),
                entries: vec![
                    Step {
                        instructions: "Slice it".into(),
                        note: None,
                    },
                    Step {
                        instructions: "Toast it".into(),
                        note: Some("Watch the edges".into()),
                    },
                    Step {
                        instructions: "Serve".into(),
                        note: None,
                    },
                ],
            }],
        };
        let SaveRecipe::Saved(id) = create(&mut first, &draft).unwrap() else {
            panic!("expected a saved recipe");
        };
        let stale = get(&mut second, id).unwrap().unwrap();
        draft.title = "Jam toast".into();
        draft.ingredients.swap(0, 1);
        draft.ingredients[1].entries[0].quantity = None;
        draft.steps[0].entries.remove(0);
        draft.steps[0].entries.swap(0, 1);
        draft.steps[0].entries[1].note = None;
        let revision = stale.revision;
        assert!(
            matches!(update(&mut first, id, revision, &draft).unwrap(), SaveRecipe::Saved(saved) if saved == id)
        );
        assert!(matches!(
            update(&mut second, id, revision, &stale.contents).unwrap(),
            SaveRecipe::Conflict
        ));

        let after = get(&mut second, id).unwrap().unwrap();
        assert_eq!(after.revision, revision + 1);
        assert_eq!(after.contents, draft);
        let mut invalid_edit = draft.clone();
        invalid_edit.steps[0].entries[0].instructions.clear();
        assert!(update(&mut second, id, after.revision, &invalid_edit).is_err());
        let after_failed_write = get(&mut first, id).unwrap().unwrap();
        assert_eq!(after_failed_write.revision, after.revision);
        assert_eq!(after_failed_write.contents, draft);
    }

    #[test]
    fn creates_a_valid_recipe_and_rolls_back_incomplete_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipes.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        let mut draft = RecipeContents {
            title: " Lemon toast ".into(),
            ingredients: vec![Section {
                heading: Some("  ".into()),
                entries: vec![Ingredient {
                    name: "Bread".into(),
                    quantity: Some(" a handful ".into()),
                }],
            }],
            steps: vec![Section {
                heading: Some("Grill".into()),
                entries: vec![Step {
                    instructions: "  ".into(),
                    note: Some("Watch the edges".into()),
                }],
            }],
        };
        assert_eq!(
            validate(draft.clone()).unwrap_err(),
            "Enter instructions for each step."
        );
        assert!(create(&mut db, &draft).is_err());
        assert!(list(&db).unwrap().is_empty());
        draft.steps[0].entries[0].instructions = "Toast it".into();
        let validated = validate(draft).unwrap();
        let SaveRecipe::Saved(id) = create(&mut db, &validated).unwrap() else {
            panic!("expected a saved recipe");
        };
        let mut reopened = crate::database::open(&path).unwrap();
        let contents = get(&mut reopened, id).unwrap().unwrap().contents;
        assert_eq!(contents, validated);
        assert_eq!(contents.title, "Lemon toast");
        assert_eq!(contents.ingredients[0].heading, None);
        assert_eq!(
            contents.ingredients[0].entries[0].quantity.as_deref(),
            Some("a handful")
        );
    }

    #[test]
    fn lists_recipes_and_reads_sections_in_saved_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipes.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        assert!(list(&db).unwrap().is_empty());
        db.execute_batch(
            "INSERT INTO recipes (id, title) VALUES (1, 'Toast'), (2, 'apple pie');
             INSERT INTO recipe_ingredient_sections VALUES (1, 1, 1, 'Topping'), (2, 1, 0, NULL);
             INSERT INTO recipe_ingredients VALUES (2, 1, 'Oil', NULL), (2, 0, 'Bread', 'a slice'),
                 (1, 0, 'Jam', NULL);
             INSERT INTO recipe_step_sections VALUES (1, 1, 1, 'Serve'), (2, 1, 0, NULL);
             INSERT INTO recipe_steps VALUES (2, 1, 'Toast it', 'Watch the edges'),
                 (2, 0, 'Slice it', NULL), (1, 0, 'Plate it', NULL);",
        )
        .unwrap();
        let cards = list(&db).unwrap();
        assert_eq!(cards.iter().map(|card| card.id).collect::<Vec<_>>(), [2, 1]);
        assert!(get(&mut db, 9).unwrap().is_none());
        let recipe = get(&mut db, 1).unwrap().unwrap().contents;
        assert_eq!(recipe.ingredients[1].heading.as_deref(), Some("Topping"));
        assert_eq!(recipe.ingredients[0].entries[0].name, "Bread");
        assert_eq!(recipe.steps[1].heading.as_deref(), Some("Serve"));
        assert_eq!(recipe.steps[0].entries[0].instructions, "Slice it");
        assert_eq!(
            recipe.steps[0].entries[1].note.as_deref(),
            Some("Watch the edges")
        );
    }
}
