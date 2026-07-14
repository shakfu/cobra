//! Loader + linker behavior against on-disk fixture mods.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use drift_mods::{load_and_link, LoadError};
use tempfile::TempDir;

fn pricing_set() -> HashSet<String> {
    HashSet::from(["supply_demand_v1".to_string()])
}

/// Write a single content file `<mod>/<subdir>/<name>.ron` with the given body.
fn write_content(root: &Path, mod_id: &str, subdir: &str, name: &str, body: &str) {
    let dir = root.join(mod_id).join(subdir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.ron")), body).unwrap();
}

fn write_manifest(root: &Path, mod_id: &str, body: &str) {
    let dir = root.join(mod_id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.toml"), body).unwrap();
}

/// A minimal, internally-consistent mod named `core`.
fn write_valid_core(root: &Path) {
    write_manifest(
        root,
        "core",
        r#"id = "core"
name = "Core"
version = "0.1.0"
"#,
    );
    write_content(
        root,
        "core",
        "commodities",
        "goods",
        r#"[
            (id: "core:food", name: "Food", base_price: 100, unit_mass: 1, elasticity: 0.8, category: "food"),
            (id: "core:ore",  name: "Ore",  base_price: 40,  unit_mass: 2, elasticity: 0.9, category: "minerals"),
        ]"#,
    );
    write_content(
        root,
        "core",
        "production",
        "recipes",
        r#"[
            (id: "core:mine_ore", inputs: [], outputs: [(commodity: "core:ore", qty: 5)], rate: 3),
        ]"#,
    );
    write_content(
        root,
        "core",
        "systems",
        "lave",
        r#"[
            (id: "core:lave", name: "Lave", position: (0.0, 0.0),
             industries: ["core:mine_ore"], connections: [],
             initial_stock: [(commodity: "core:food", qty: 100), (commodity: "core:ore", qty: 50)],
             pricing: "supply_demand_v1"),
        ]"#,
    );
    write_content(
        root,
        "core",
        "ships",
        "cobra",
        r#"[
            (id: "core:cobra_mk3", name: "Cobra Mk III", cargo_capacity: 35, jump_speed: 7.0, hull: 100, max_speed: 350.0),
        ]"#,
    );
}

#[test]
fn valid_mod_loads_and_links() {
    let tmp = TempDir::new().unwrap();
    write_valid_core(tmp.path());

    let reg = load_and_link(tmp.path(), &pricing_set()).expect("should link");
    assert_eq!(reg.commodity_count(), 2);
    assert_eq!(reg.system_count(), 1);
    assert!(reg.commodity_id("core:food").is_some());
    assert!(reg.ship_id("core:cobra_mk3").is_some());
    // The system's industry resolved to the mining recipe.
    let sys = reg.systems().next().unwrap();
    assert_eq!(sys.industries.len(), 1);
    assert_eq!(reg.recipe(sys.industries[0]).outputs.len(), 1);
}

#[test]
fn missing_dependency_errors() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        tmp.path(),
        "a",
        r#"id = "a"
name = "A"
version = "0.1.0"
dependencies = ["b"]
"#,
    );
    let err = load_and_link(tmp.path(), &pricing_set()).unwrap_err();
    assert!(
        matches!(err, LoadError::MissingDependency { ref mod_id, ref dependency } if mod_id == "a" && dependency == "b"),
        "got {err:?}"
    );
}

#[test]
fn dependency_cycle_errors() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        tmp.path(),
        "a",
        "id = \"a\"\nname = \"A\"\nversion = \"0.1.0\"\ndependencies = [\"b\"]\n",
    );
    write_manifest(
        tmp.path(),
        "b",
        "id = \"b\"\nname = \"B\"\nversion = \"0.1.0\"\ndependencies = [\"a\"]\n",
    );
    let err = load_and_link(tmp.path(), &pricing_set()).unwrap_err();
    assert!(matches!(err, LoadError::DependencyCycle(_)), "got {err:?}");
}

#[test]
fn dangling_commodity_reference_errors() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        tmp.path(),
        "core",
        "id = \"core\"\nname = \"Core\"\nversion = \"0.1.0\"\n",
    );
    // Recipe outputs a commodity that is never defined.
    write_content(
        tmp.path(),
        "core",
        "production",
        "recipes",
        r#"[ (id: "core:bad", inputs: [], outputs: [(commodity: "core:ghost", qty: 1)], rate: 1) ]"#,
    );
    let err = load_and_link(tmp.path(), &pricing_set()).unwrap_err();
    assert!(
        matches!(err, LoadError::DanglingRef { ref target, .. } if target == "core:ghost"),
        "got {err:?}"
    );
}

#[test]
fn unknown_pricing_strategy_errors() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        tmp.path(),
        "core",
        "id = \"core\"\nname = \"Core\"\nversion = \"0.1.0\"\n",
    );
    write_content(
        tmp.path(),
        "core",
        "commodities",
        "goods",
        r#"[ (id: "core:food", name: "Food", base_price: 100, unit_mass: 1, elasticity: 0.8, category: "food") ]"#,
    );
    write_content(
        tmp.path(),
        "core",
        "systems",
        "lave",
        r#"[ (id: "core:lave", name: "Lave", position: (0.0, 0.0), industries: [], connections: [],
             initial_stock: [(commodity: "core:food", qty: 10)], pricing: "does_not_exist") ]"#,
    );
    let err = load_and_link(tmp.path(), &pricing_set()).unwrap_err();
    assert!(
        matches!(err, LoadError::UnknownPricing { ref strategy, .. } if strategy == "does_not_exist"),
        "got {err:?}"
    );
}

#[test]
fn duplicate_id_without_override_errors() {
    let tmp = TempDir::new().unwrap();
    write_valid_core(tmp.path());
    // A second mod redefines core:food but does NOT declare an override.
    write_manifest(
        tmp.path(),
        "patch",
        "id = \"patch\"\nname = \"Patch\"\nversion = \"0.1.0\"\ndependencies = [\"core\"]\n",
    );
    write_content(
        tmp.path(),
        "patch",
        "commodities",
        "food",
        r#"[ (id: "core:food", name: "Food+", base_price: 200, unit_mass: 1, elasticity: 0.8, category: "food") ]"#,
    );
    let err = load_and_link(tmp.path(), &pricing_set()).unwrap_err();
    assert!(
        matches!(err, LoadError::DuplicateId { ref id, .. } if id == "core:food"),
        "got {err:?}"
    );
}

#[test]
fn declared_override_replaces_definition() {
    let tmp = TempDir::new().unwrap();
    write_valid_core(tmp.path());
    write_manifest(
        tmp.path(),
        "patch",
        "id = \"patch\"\nname = \"Patch\"\nversion = \"0.1.0\"\ndependencies = [\"core\"]\noverrides = [\"core:food\"]\n",
    );
    write_content(
        tmp.path(),
        "patch",
        "commodities",
        "food",
        r#"[ (id: "core:food", name: "Food+", base_price: 200, unit_mass: 1, elasticity: 0.8, category: "food") ]"#,
    );
    let reg = load_and_link(tmp.path(), &pricing_set()).expect("override should link");
    let food = reg.commodity_id("core:food").unwrap();
    assert_eq!(reg.commodity(food).base_price, 200, "override took effect");
    assert_eq!(reg.commodity(food).name, "Food+");
    // Override replaces in place; the commodity count is unchanged.
    assert_eq!(reg.commodity_count(), 2);
}
