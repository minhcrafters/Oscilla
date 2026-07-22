use super::theme::*;
use super::Message;
use nice_plug_iced::iced::{
    Background, Border, Color, Element, Length, Shadow,
    widget::{
        Space, button, column, container, row, scrollable, text, text_input,
    },
};

pub enum PopupState {
    Save {
        name: String,
    },
    Load {
        files: Vec<String>,
        selected: Option<String>,
    },
}

pub fn popup_view(popup: &PopupState) -> Element<'static, Message> {
    let card = match popup {
        PopupState::Save { name } => {
            let input = text_input("Preset name", name)
                .on_input(Message::SaveNameChanged)
                .on_submit(Message::ConfirmSave)
                .padding(8)
                .size(14);

            let buttons = row![
                Space::new().width(Length::Fill),
                popup_button("Cancel", Message::ClosePopup),
                popup_button("Save", Message::ConfirmSave),
            ]
            .spacing(8);

            column![heading("Save Preset"), input, buttons]
                .spacing(12)
                .width(300)
        }
        PopupState::Load { files, selected } => {
            let mut list = column![].spacing(2);
            if files.is_empty() {
                list = list.push(text("No presets found").size(13).color(FG_DIM));
            }
            for f in files.iter() {
                let name = f.clone();
                let is_selected = selected.as_deref() == Some(&name);
                list = list.push(
                    button(text(name.clone()).size(13).color(if is_selected {
                        ACCENT
                    } else {
                        FG_TEXT
                    }))
                    .on_press(Message::SelectPreset(name))
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(move |_theme, _status| button::Style {
                        background: if is_selected {
                            Some(Background::Color(ACCENT_SOFT))
                        } else {
                            None
                        },
                        border: Border::default(),
                        text_color: if is_selected { ACCENT } else { FG_TEXT },
                        shadow: Shadow::default(),
                        snap: false,
                    }),
                );
            }

            let buttons = row![
                Space::new().width(Length::Fill),
                popup_button("Cancel", Message::ClosePopup),
                popup_button("Load", Message::ConfirmLoad),
            ]
            .spacing(8);

            let scroll = scrollable(
                container(list)
                    .style(|_| container::Style {
                        background: Some(Background::Color(BG_DEEP)),
                        border: Border {
                            color: BORDER,
                            width: 1.0,
                            radius: rad(3.0),
                        },
                        ..Default::default()
                    })
                    .padding(8)
                    .width(Length::Fill),
            )
            .height(250);

            column![heading("Load Preset"), scroll, buttons]
                .spacing(12)
                .width(300)
        }
    };

    let card = container(card)
        .style(|_| container::Style {
            background: Some(Background::Color(SURFACE)),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: rad(6.0),
            },
            ..Default::default()
        })
        .padding(20);

    container(column![
        Space::new().height(Length::Fill),
        row![
            Space::new().width(Length::Fill),
            card,
            Space::new().width(Length::Fill),
        ]
        .height(Length::Shrink),
        Space::new().height(Length::Fill),
    ])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
        ..Default::default()
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn popup_button(label: &str, msg: Message) -> button::Button<'_, Message> {
    button(btn_text(label))
        .on_press(msg)
        .padding([6, 18])
        .style(accent_btn_style)
}

pub fn preset_dir() -> Option<std::path::PathBuf> {
    Some(dirs::document_dir()?.join("Oscilla"))
}

pub fn list_presets() -> Vec<String> {
    let dir = match preset_dir() {
        Some(d) => d,
        None => return vec![],
    };
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".osc") {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}
