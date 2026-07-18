// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    macinmeter_gui::run()
}
