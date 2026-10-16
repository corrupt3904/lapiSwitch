use super::view::*;
use super::*;
use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Border, Color, Fill};

pub(super) fn modal(app: &App) -> iced::widget::Container<'_, Message> {
    let catalog = crate::licenses::catalog();
    let mut choices = column![].spacing(4);
    let mut count = 0;
    for (index, component) in catalog.components.iter().enumerate() {
        if !component.matches(&app.license_search) {
            continue;
        }
        count += 1;
        let selected = index == app.license_selection;
        choices = choices.push(
            button(
                column![
                    label(&component.name).size(14),
                    small(format!("{} · {}", component.version, component.license)).size(11),
                ]
                .spacing(4),
            )
            .on_press(Message::LicenseSelect(index))
            .padding(10)
            .width(Fill)
            .style(move |theme, status| {
                let mut style = soft_style(theme, status);
                if selected {
                    style.background = Some(PALE.into());
                    style.border.color = ACCENT;
                }
                style
            }),
        );
    }
    if count == 0 {
        choices = choices.push(small(
            app.t("검색 결과가 없습니다.", "No matching components."),
        ));
    }
    let selected = &catalog.components[app.license_selection];
    let mut documents = column![
        label(&selected.name).size(22),
        small(format!("{} · {}", selected.version, selected.license)),
        small(&selected.source)
            .width(Fill)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(12);
    for notice in &selected.notices {
        documents = documents
            .push(label(&notice.title).size(15))
            // Original notices remain verbatim; don't translate or normalize paths.
            .push(
                text(catalog.texts[notice.text].as_str())
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .size(13)
                    .width(Fill)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .color(INK),
            );
    }
    let reader = container(
        scrollable(container(documents).width(Fill).padding([0, 12]))
            .id(format!("license-document-{}", app.license_selection))
            .width(Fill)
            .height(Fill),
    )
    .padding(16)
    .width(Fill)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(Color::WHITE.into()),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });
    panel(
        column![
            label(app.t("오픈소스 라이선스", "Open-source licenses")).size(25),
            small(app.t(
                "구성요소를 선택하면 라이선스와 저작권 고지 원문을 볼 수 있습니다. 인터넷 연결은 필요하지 않습니다.",
                "Select a component to read its original license and copyright notices. No internet connection is required."
            )),
            row![
                column![
                    text_input(
                        app.t("구성요소·라이선스 검색", "Search components or licenses"),
                        &app.license_search
                    )
                    .on_input(Message::LicenseSearch)
                    .padding(10),
                    scrollable(choices).height(Fill),
                ]
                .spacing(12)
                .width(230),
                reader,
            ]
            .spacing(16)
            .height(Fill),
            row![
                space().width(Fill),
                action(app.t("닫기", "Close"), Message::LicenseClose, true).style(soft_style)
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(16),
    )
    .max_width(980)
    .height(iced::Length::FillPortion(9))
    .max_height(700)
}
