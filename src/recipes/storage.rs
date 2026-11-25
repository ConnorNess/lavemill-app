use super::{Ingredient, Recipe, RecipeCard, RecipeContents, Section, Step};
use rusqlite::{Connection, OptionalExtension, Result, Row};

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
    let ingredients = read_sections(
        &tx,
        id,
        "SELECT id, heading FROM recipe_ingredient_sections WHERE recipe_id = ?1 ORDER BY position",
        "SELECT name, quantity FROM recipe_ingredients WHERE section_id = ?1 ORDER BY position",
        |row| {
            Ok(Ingredient {
                name: row.get(0)?,
                quantity: row.get(1)?,
            })
        },
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
