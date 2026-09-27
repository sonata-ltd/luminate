use iced::widget::svg::Handle;
use std::sync::LazyLock;

macro_rules! icon {
    ($name:ident, $file:literal) => {
        pub(crate) static $name: LazyLock<Handle> =
            LazyLock::new(|| Handle::from_memory(include_bytes!($file).as_slice()));
    };
}

icon!(ARROW_RIGHT, "arrow-right.svg");
icon!(DOWNLOAD_03, "download-03.svg");
icon!(SETTINGS_02, "settings-02.svg");
