fn main() {
    if gitgui_lib::helper::entry() {
        return;
    }
    gitgui_lib::run();
}
