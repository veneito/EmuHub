#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod appearance;
mod assets;
mod emu;
mod gamepad;

use emu::{Config, Game, System};
use gamepad::Pad;
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Root, Selectable, Theme, ThemeMode, TitleBar,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};

const MUTED: u32 = 0xa7b4ba;
const TEXT: u32 = 0xeeeae2;
const FONT: &str = if cfg!(target_os = "windows") { "Segoe UI" } else { ".SystemUIFont" };

#[derive(Clone, Copy, PartialEq)]
enum View {
    Library,
    Settings,
}

struct Hub {
    query: Entity<InputState>,
    roms_dir: Entity<InputState>,
    retroarch_cmd: Entity<InputState>,
    cores_dir: Entity<InputState>,
    steamgriddb_key: Entity<InputState>,
    config: Config,
    systems: Vec<System>,
    games: Vec<Game>,
    view: View,
    system: Option<String>,
    selected: Option<usize>,
    sidebar_open: bool,
    busy: bool,
    /// Controller input is ignored while another window (e.g. a running game) has focus.
    active: bool,
    /// Cards per grid row, measured each frame for up/down navigation.
    columns: usize,
    grid_scroll: ScrollHandle,
    message: String,
    appearance: appearance::Appearance,
}

// The look below is shared with Project Launcher: icon(), tile(), heading(), card(), setting_row().
fn icon(name: &str, size: f32, color: impl Into<Hsla>) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .w(px(size))
        .h(px(size))
        .flex_none()
        .text_color(color)
}

/// A rounded square with the first letter, colored from the name.
fn tile(name: &str, size: f32) -> Div {
    let hue = (name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360) as f32 / 360.;
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(hue, 0.38, 0.28, 1.))
        .text_color(hsla(hue, 0.7, 0.85, 1.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size * 0.42))
        .child(name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
}

fn heading(text: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text.to_uppercase())
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(rgba(0xffffff14))
        .bg(rgba(0xffffff06))
}

/// One settings line: title and note on the left, add the control as a child.
fn setting_row(title: &str, note: &str) -> Div {
    let mut text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(15.)).child(title.to_owned()));
    if !note.is_empty() {
        text = text.child(div().text_xs().text_color(rgb(MUTED)).child(note.to_owned()));
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .px_5()
        .py_3()
        .border_b_1()
        .border_color(rgba(0xffffff0f))
        .child(text)
}

fn system_name<'a>(systems: &'a [System], id: &'a str) -> &'a str {
    systems.iter().find(|s| s.id == id).map_or(id, |s| s.name.as_str())
}

impl Hub {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = appearance::Appearance::load();
        appearance.apply(window);
        appearance.apply_theme(cx);
        let config = Config::load();
        let mut field = |value: String| cx.new(|cx| InputState::new(window, cx).default_value(value));
        let roms_dir = field(config.roms_dir.display().to_string());
        // shortcut: splits on spaces, so a RetroArch path containing spaces won't work; add a proper args field if needed.
        let retroarch_cmd = field(config.retroarch_cmd.join(" "));
        let cores_dir = field(config.cores_dir.display().to_string());
        let steamgriddb_key = field(config.steamgriddb_key.clone());
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search games"));
        cx.subscribe_in(&query, window, |view, _, event, _, cx| match event {
            InputEvent::PressEnter { .. } => {
                if let Some(&i) = view.visible(cx).first() {
                    view.selected = Some(i);
                    view.play(i, cx);
                }
            }
            InputEvent::Change => cx.notify(),
            _ => {}
        })
        .detach();
        cx.observe_window_activation(window, |view, window, _| view.active = window.is_window_active()).detach();
        let pad = gamepad::spawn();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let actions = std::mem::take(&mut *pad.lock().unwrap());
                if !actions.is_empty() && this.update(cx, |view, cx| actions.into_iter().for_each(|a| view.on_pad(a, cx))).is_err() {
                    return;
                }
            }
        })
        .detach();

        let mut hub = Self {
            query,
            roms_dir,
            retroarch_cmd,
            cores_dir,
            steamgriddb_key,
            systems: emu::systems(),
            games: vec![],
            config,
            view: View::Library,
            system: None,
            selected: None,
            sidebar_open: true,
            busy: false,
            active: true,
            columns: 1,
            grid_scroll: ScrollHandle::new(),
            message: String::new(),
            appearance,
        };
        hub.rescan(cx);
        hub
    }

    /// Indexes into `games` that pass the system filter and search box.
    fn visible(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().to_lowercase();
        (0..self.games.len())
            .filter(|&i| {
                let g = &self.games[i];
                self.system.as_deref().is_none_or(|s| s == g.system) && g.name.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.games = emu::scan(&self.config.roms_dir, &self.systems);
        self.selected = None;
        if self.system.as_deref().is_some_and(|s| !self.games.iter().any(|g| g.system == s)) {
            self.system = None;
        }
        if self.games.is_empty() {
            self.message = "No games found. Open Settings, create the ROM folders, and put your games in them.".into();
        }
        self.fetch_covers(cx);
        cx.notify();
    }

    /// Downloads missing covers one by one in the background, redrawing as each arrives.
    fn fetch_covers(&self, cx: &mut Context<Self>) {
        let games = self.games.clone();
        let key = self.config.steamgriddb_key.clone();
        cx.spawn(async move |this, cx| {
            for game in games {
                let key = key.clone();
                let got = cx.background_executor().spawn(async move { emu::fetch_cover(&game, &key) }).await;
                if got && this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn play(&mut self, index: usize, cx: &mut Context<Self>) {
        let game = &self.games[index];
        self.message = match emu::launch(&self.config, game) {
            Ok(()) => format!("Launched {}", game.name),
            Err(error) => error,
        };
        cx.notify();
    }

    fn on_pad(&mut self, action: Pad, cx: &mut Context<Self>) {
        if !self.active || !gamepad::app_in_front() {
            return;
        }
        match action {
            Pad::PrevTab | Pad::NextTab => {
                let mut tabs: Vec<Option<String>> = vec![None];
                tabs.extend(self.systems.iter().filter(|s| self.games.iter().any(|g| g.system == s.id)).map(|s| Some(s.id.clone())));
                let at = tabs.iter().position(|t| *t == self.system).unwrap_or(0);
                let next = if action == Pad::NextTab { (at + 1) % tabs.len() } else { (at + tabs.len() - 1) % tabs.len() };
                self.system = tabs[next].clone();
                self.view = View::Library;
                self.selected = None;
            }
            Pad::Back if self.view == View::Settings => self.view = View::Library,
            Pad::Back => {
                self.system = None;
                self.selected = None;
            }
            _ if self.view != View::Library => return,
            Pad::Confirm => match self.selected {
                Some(i) => self.play(i, cx),
                None => self.move_selection(0, cx),
            },
            Pad::Left => self.move_selection(-1, cx),
            Pad::Right => self.move_selection(1, cx),
            Pad::Up => self.move_selection(-(self.columns as isize), cx),
            Pad::Down => self.move_selection(self.columns as isize, cx),
        }
        cx.notify();
    }

    /// Moves the selection through the visible games, starting at the first one if nothing is selected.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let visible = self.visible(cx);
        if visible.is_empty() {
            return;
        }
        let pos = match self.selected.and_then(|i| visible.iter().position(|&v| v == i)) {
            Some(pos) => (pos as isize + by).clamp(0, visible.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(visible[pos]);
        self.grid_scroll.scroll_to_item(pos);
    }

    /// Runs a slow job (install, download) off the UI thread and shows its result.
    fn run(&mut self, label: &str, job: impl FnOnce(Config) -> Result<String, String> + Send + 'static, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = label.into();
        let config = self.config.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(config) }).await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                view.message = result.unwrap_or_else(|e| e);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        self.config.roms_dir = read(&self.roms_dir).into();
        self.config.retroarch_cmd = read(&self.retroarch_cmd).split_whitespace().map(String::from).collect();
        self.config.cores_dir = read(&self.cores_dir).into();
        self.config.steamgriddb_key = read(&self.steamgriddb_key);
        self.message = match self.config.save() {
            Ok(()) => "Settings saved.".into(),
            Err(error) => error,
        };
        self.rescan(cx);
    }

    // ---- navigation -------------------------------------------------------

    fn nav_row(&self, id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .h(px(40.))
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(15.))
            .text_color(rgb(if selected { 0xf5f3ee } else { 0xc3cdd1 }))
            .bg(if selected { rgba((self.appearance.accent << 8) | 0x30) } else { rgba(0x00000000) })
            .hover(|style| style.bg(rgba(0xffffff12)))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let tone = |selected: bool| rgb(if selected { 0xf5f3ee } else { MUTED });
        let all = self.view == View::Library && self.system.is_none();
        let settings = self.view == View::Settings;
        let mut nav = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                self.nav_row("nav-all", all)
                    .child(icon("library", 18., tone(all)))
                    .child(div().flex_1().child("All games"))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(self.games.len().to_string()))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Library;
                        view.system = None;
                        cx.notify();
                    })),
            )
            .child(div().px_3().pt_3().pb_1().child(heading("Your Library")));
        for system in &self.systems {
            let count = self.games.iter().filter(|g| g.system == system.id).count();
            if count == 0 {
                continue;
            }
            let selected = self.view == View::Library && self.system.as_deref() == Some(system.id.as_str());
            let pick = system.id.clone();
            nav = nav.child(
                self.nav_row(SharedString::from(format!("sys-{}", system.id)), selected)
                    .child(tile(&system.name, 24.))
                    .child(div().flex_1().truncate().child(system.name.clone()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(count.to_string()))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.view = View::Library;
                        view.system = Some(pick.clone());
                        cx.notify();
                    })),
            );
        }
        div()
            .id("sidebar")
            .w(px(256.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .bg(self.appearance.fill(0x081012))
            .p_3()
            .flex()
            .flex_col()
            .justify_between()
            .child(nav)
            .child(
                self.nav_row("nav-settings", settings)
                    .child(icon("settings", 18., tone(settings)))
                    .child("Settings")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Settings;
                        cx.notify();
                    })),
            )
    }

    // ---- pages ------------------------------------------------------------

    fn library_page(&self, visible: &[usize], cx: &mut Context<Self>) -> Stateful<Div> {
        let title = match &self.system {
            Some(id) => system_name(&self.systems, id).to_owned(),
            None => "All games".to_owned(),
        };
        let accent = self.appearance.accent;
        let mut grid = div()
            .id("grid")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.grid_scroll)
            .flex()
            .flex_wrap()
            .content_start()
            .gap_5()
            .px_6()
            .pb_6();
        for &index in visible {
            let game = &self.games[index];
            let cover = emu::cover_path(game);
            // shortcut: checks the disk on every redraw; keep a set of known covers if libraries get huge.
            let art = if cover.exists() {
                img(cover).size_full().object_fit(ObjectFit::Cover).into_any_element()
            } else {
                tile(&game.name, 150.).size_full().rounded_none().into_any_element()
            };
            let selected = self.selected == Some(index);
            grid = grid.child(
                div()
                    .id(("game", index))
                    .w(px(150.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                        view.selected = Some(index);
                        if event.click_count() >= 2 {
                            view.play(index, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(150.))
                            .h(px(225.))
                            .rounded_lg()
                            .overflow_hidden()
                            .border_2()
                            .border_color(if selected { rgb(accent).into() } else { rgba(0xffffff14) })
                            .child(art),
                    )
                    .child(div().text_size(px(14.)).line_clamp(2).child(game.name.clone()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(system_name(&self.systems, &game.system).to_owned())),
            );
        }
        if visible.is_empty() {
            grid = grid.child(div().text_color(rgb(MUTED)).child("No games here."));
        }
        div()
            .id("library")
            .size_full()
            .bg(self.appearance.fill(0x0c1417))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_6()
                    .p_6()
                    .child(tile(&title, 128.).rounded_lg())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(heading("Library"))
                            .child(div().text_size(px(34.)).font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(div().text_color(rgb(MUTED)).child(format!(
                                "{} game{}",
                                visible.len(),
                                if visible.len() == 1 { "" } else { "s" }
                            )))
                            .child(
                                div().pt_2().child(
                                    Button::new("rescan")
                                        .label("Rescan")
                                        .on_click(cx.listener(|view, _, _, cx| view.rescan(cx))),
                                ),
                            ),
                    ),
            )
            .child(div().px_6().pb_4().child(Input::new(&self.query)))
            .child(grid)
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let paths = card()
            .child(setting_row("ROMs folder", "One subfolder per system, EmuDeck style (roms/snes, roms/gba…).").child(div().w(px(360.)).child(Input::new(&self.roms_dir))))
            .child(setting_row("RetroArch command", "Program and arguments, separated by spaces.").child(div().w(px(360.)).child(Input::new(&self.retroarch_cmd))))
            .child(setting_row("Cores folder", "Where RetroArch keeps its cores.").child(div().w(px(360.)).child(Input::new(&self.cores_dir))))
            .child(setting_row("SteamGridDB API key", "Optional. Fills in covers libretro doesn't have. steamgriddb.com → Preferences → API.").child(div().w(px(360.)).child(Input::new(&self.steamgriddb_key))))
            .child(setting_row("Save changes", "").child(
                Button::new("save").primary().label("Save").on_click(cx.listener(|view, _, _, cx| view.save_settings(cx))),
            ));

        let setup = card()
            .child(if self.config.roms_dir.is_dir() {
                setting_row("ROM folders", "Put each game in its system's folder, then Rescan.").child(
                    Button::new("open-folders").label("Open").on_click(cx.listener(|view, _, _, cx| {
                        view.message = emu::open_folder(&view.config.roms_dir).err().unwrap_or_default();
                        cx.notify();
                    })),
                )
            } else {
                setting_row("ROM folders", "Creates a folder for every supported system.").child(
                    Button::new("create-folders").label("Create").on_click(cx.listener(|view, _, _, cx| {
                        view.message = match emu::create_folders(&view.config) {
                            Ok(()) => format!("Created folders in {}", view.config.roms_dir.display()),
                            Err(error) => error,
                        };
                        view.rescan(cx);
                    })),
                )
            })
            .child(if emu::retroarch_installed(&self.config) {
                setting_row("RetroArch", "Installed. Open it to change its own settings.").child(
                    Button::new("open-retroarch").label("Open").on_click(cx.listener(|view, _, _, cx| {
                        view.message = emu::open_retroarch(&view.config).err().unwrap_or_default();
                        cx.notify();
                    })),
                )
            } else {
                setting_row("RetroArch", "Not installed. Installs with winget, Homebrew or Flatpak.").child(
                    Button::new("install-retroarch").label("Install").disabled(self.busy).on_click(cx.listener(|view, _, _, cx| {
                        view.run("Installing RetroArch…", |_| emu::install_retroarch().map(|()| "RetroArch installed.".into()), cx)
                    })),
                )
            })
            .child({
                let (missing, total) = emu::core_status(&self.config);
                let note = if missing == 0 {
                    format!("All {total} cores installed.")
                } else {
                    format!("{missing} of {total} cores missing. Downloads them from libretro.")
                };
                setting_row("Cores", &note).child(
                    Button::new("install-cores")
                        .label(if missing == 0 { "Installed" } else { "Install" })
                        .disabled(self.busy || missing == 0)
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.run("Downloading cores…", |config| emu::install_cores(&config).map(|n| format!("Installed {n} new cores.")), cx)
                        })),
                )
            });

        let mut modes = div().flex().gap_2();
        for (label, glass) in [("Solid", false), ("Glass", true)] {
            modes = modes.child(
                Button::new(label)
                    .label(label)
                    .selected(self.appearance.glass == glass)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.appearance.glass = glass;
                        view.save_appearance(window, cx);
                    })),
            );
        }
        let mut palettes = div().flex().flex_wrap().justify_end().gap_2();
        for (name, surface, accent) in [
            ("Teal", 0x0c1417, 0x92c8e5),
            ("Graphite", 0x151515, 0xc4c4ce),
            ("Blue", 0x101a2d, 0x92b9ff),
            ("Plum", 0x211324, 0xd5a8e3),
            ("Forest", 0x101f19, 0x91cba6),
        ] {
            palettes = palettes.child(
                Button::new(name)
                    .label(name)
                    .selected(self.appearance.surface == surface)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.appearance.surface = surface;
                        view.appearance.accent = accent;
                        view.save_appearance(window, cx);
                    })),
            );
        }
        let looks = card()
            .child(setting_row("Window material", "Glass blurs the desktop behind the window (Windows and macOS).").child(modes))
            .child(setting_row("Theme", "Preset colors for the whole window.").child(palettes));

        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .bg(self.appearance.fill(0x0c1417))
            .p_8()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_size(px(34.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
            .child(div().pt_4().child(heading("Paths")))
            .child(paths)
            .child(div().pt_4().child(heading("Setup")))
            .child(setup)
            .child(div().pt_4().child(heading("Appearance")))
            .child(looks)
    }

    fn save_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance.apply(window);
        self.appearance.apply_theme(cx);
        self.message = match self.appearance.save() {
            Ok(()) => "Appearance saved.".into(),
            Err(error) => error,
        };
        cx.notify();
    }

    fn bottom_bar(&self, cx: &mut Context<Self>) -> Div {
        let focus = self.selected.map(|i| &self.games[i]);
        let left = match focus {
            Some(game) => div()
                .w(px(300.))
                .flex()
                .items_center()
                .gap_3()
                .child(tile(&game.name, 44.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(15.)).child(game.name.clone()))
                        .child(div().truncate().text_xs().text_color(rgb(MUTED)).child(system_name(&self.systems, &game.system).to_owned())),
                ),
            None => div().w(px(300.)).text_color(rgb(MUTED)).child("Nothing selected"),
        };
        div()
            .h(px(76.))
            .flex_shrink_0()
            .px_5()
            .bg(self.appearance.fill(0x101e23))
            .border_t_1()
            .border_color(rgba(0xffffff14))
            .flex()
            .items_center()
            .gap_4()
            .child(left)
            .child(
                div().flex_1().flex().justify_center().child(
                    Button::new("play")
                        .primary()
                        .label("Play")
                        .disabled(focus.is_none())
                        .on_click(cx.listener(|view, _, _, cx| {
                            if let Some(i) = view.selected {
                                view.play(i, cx);
                            }
                        })),
                ),
            )
            .child(div().w(px(300.)).text_xs().text_right().text_color(rgb(MUTED)).child(if self.message.is_empty() {
                format!("{} games", self.games.len())
            } else {
                self.message.clone()
            }))
    }
}

impl Render for Hub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sidebar, window padding, border and grid padding; each card is 150px plus a 20px gap.
        let chrome = if self.sidebar_open { 256. } else { 0. } + 16. + 4. + 2. + 48.;
        let content = window.viewport_size().width / px(1.) - chrome;
        self.columns = ((content + 20.) / 170.).floor().max(1.) as usize;
        let visible = self.visible(cx);
        let page = match self.view {
            View::Library => self.library_page(&visible, cx),
            View::Settings => self.settings_page(cx),
        };
        let mut body = div().flex_1().min_h_0().flex().p_2().gap_1();
        if self.sidebar_open {
            body = body.child(self.sidebar(cx));
        }
        body = body.child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded_lg()
                .overflow_hidden()
                .border_1()
                .border_color(rgba(0xffffff14))
                .child(page),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(if self.appearance.glass { rgba(0x00000000) } else { self.appearance.fill(0x081012) })
            .font_family(FONT)
            .text_sm()
            .text_color(rgb(TEXT))
            .child(
                TitleBar::new().bg(self.appearance.fill(0x081012)).child(
                    div().flex_1().flex().items_center().gap_2().child(
                        div()
                            .id("toggle-sidebar")
                            .w(px(32.))
                            .h(px(32.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgba(0xffffff1a)))
                            .child(icon("sidebar", 18., rgb(if self.sidebar_open { TEXT } else { MUTED })))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sidebar_open = !view.sidebar_open;
                                cx.notify();
                            })),
                    )
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("EmuHub")),
                ),
            )
            .child(body)
            .child(self.bottom_bar(cx))
    }
}

fn main() {
    Application::new().with_assets(assets::Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        Theme::global_mut(cx).colors.background = rgba(0x00000000).into();
        let bounds = Bounds::centered(None, size(px(1240.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(860.), px(560.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("EmuHub".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Hub::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("Could not open the EmuHub window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
