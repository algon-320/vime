lazy_static::lazy_static! {
    pub static ref CONFIG: Config = vime_config();
}

fn vime_config() -> Config {
    if std::env::var("VIME_EDITOR").is_err() {
        let vim_path = find_vim();

        let home = std::env::var("HOME").expect("$HOME is not defined");
        let custom_vimrc = format!("{}/.config/vime/vimrc", home);

        std::env::set_var(
            "VIME_EDITOR",
            format!("{} -u {custom_vimrc}", vim_path.display()),
        );
    }

    let editor = std::env::var("VIME_EDITOR").unwrap();
    let mut cmd: Vec<String> = editor.split(' ').map(|s| s.to_owned()).collect();
    cmd.push("/tmp/vime_buffer.txt".to_owned());

    let mut config = build();
    config.shell = cmd;
    config
}

fn find_vim() -> std::path::PathBuf {
    // FIXME: search $PATH directories for the vim binary
    std::path::PathBuf::from("/usr/bin/vim")
}

use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub default_columns: usize,
    pub default_rows: usize,

    pub trigger_key_state: u32,
    pub trigger_key_keycode: u8,

    ///////////////////////////// toyterm ////////////////////////////
    pub shell: Vec<String>,

    // paths to font files which FreeType supports (TTF, OTF, etc.)
    pub fonts_regular: Vec<PathBuf>,
    pub fonts_bold: Vec<PathBuf>,
    pub fonts_faint: Vec<PathBuf>,
    pub font_size: u32,

    pub status_bar_font_size: u32,

    // RRGGBBAA
    pub color_black: u32,
    pub color_red: u32,
    pub color_green: u32,
    pub color_yellow: u32,
    pub color_blue: u32,
    pub color_magenta: u32,
    pub color_cyan: u32,
    pub color_white: u32,
    pub color_bright_black: u32,
    pub color_bright_red: u32,
    pub color_bright_green: u32,
    pub color_bright_yellow: u32,
    pub color_bright_blue: u32,
    pub color_bright_magenta: u32,
    pub color_bright_cyan: u32,
    pub color_bright_white: u32,

    pub scroll_bar_width: u32,
    pub scroll_bar_fg_color: u32,
    pub scroll_bar_bg_color: u32,

    pub east_asian_width_ambiguous: u8,
}

impl Default for Config {
    fn default() -> Self {
        let shell = vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())];

        Config {
            default_columns: 80,
            default_rows: 24,

            trigger_key_state: 0x8,  // Alt
            trigger_key_keycode: 62, // RightShift

            shell,

            east_asian_width_ambiguous: 1,

            // FIXME: due to a bug on "config-rs", empty Vecs cannot be serialized properly.
            // https://github.com/mehcode/config-rs/issues/114
            fonts_regular: vec![PathBuf::new()],
            fonts_bold: vec![PathBuf::new()],
            fonts_faint: vec![PathBuf::new()],
            font_size: 32,

            status_bar_font_size: 32,

            scroll_bar_width: 5,
            scroll_bar_fg_color: 0x606060FF,
            scroll_bar_bg_color: 0x202020FF,

            color_black: 0x000000FF,
            color_red: 0xFF0000FF,
            color_green: 0x00FF00FF,
            color_yellow: 0xFFFF00FF,
            color_blue: 0x0000FFFF,
            color_magenta: 0xFF00FFFF,
            color_cyan: 0x00FFFFFF,
            color_white: 0xFFFFFFFF,

            color_bright_black: 0x505050FF,
            color_bright_red: 0xFF5050FF,
            color_bright_green: 0x50FF50FF,
            color_bright_yellow: 0xFFFF50FF,
            color_bright_blue: 0x5050FFFF,
            color_bright_magenta: 0xFF50FFFF,
            color_bright_cyan: 0x50FFFFFF,
            color_bright_white: 0xFFFFFFFF,
        }
    }
}

fn build() -> Config {
    let mut builder = ::config::Config::builder();

    // default config
    let default_config = Config::default();
    let default_source = ::config::Config::try_from(&default_config).unwrap();
    builder = builder.add_source(default_source);

    // user config
    if let Some(config_path) = find_config_file() {
        builder = builder.add_source(config::File::from(config_path).required(false));
    }

    builder
        .build()
        .unwrap()
        .try_deserialize()
        .expect("Failed to build config")
}

fn find_config_file() -> Option<PathBuf> {
    let mut xdg_config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            // fallback to "$HOME/.config"
            let home = std::env::var_os("HOME")?;
            let mut p = PathBuf::from(home);
            p.push(".config");
            Some(p)
        })?;

    xdg_config_home.push("vime");
    xdg_config_home.push("config.toml");
    Some(xdg_config_home)
}
