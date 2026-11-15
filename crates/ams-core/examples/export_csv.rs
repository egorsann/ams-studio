//! cargo run -p ams-core --example export_csv -- file.ams > out.csv
//! Same output as File > Export Data File in the program (in-phase and out-of-phase side by side).
use ams_core::{ams, export};

fn main() {
    let path = std::env::args().nth(1).expect("usage: export_csv file.ams");
    let specs = ams::read_file(std::path::Path::new(&path)).expect("read ams");
    let rows: Vec<&_> = specs.iter().collect();
    print!("{}", export::csv_both(&rows));
}
