fn main() {
    // sqlx's embedded migrator must rebuild when a new migration file appears.
    println!("cargo:rerun-if-changed=../../migrations");
}
