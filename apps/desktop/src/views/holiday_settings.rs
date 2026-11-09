//! Settings → Calendar → Holidays: on/off, observances, "where you are",
//! and a searchable multi-select of countries.

use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, SharedString, Styled as _, Subscription,
    Window, px, rems,
};
use keepstone_core::holidays;
use keepstone_ui::appearance;

/// A country in the picker; searchable by name or code ("in", "India").
#[derive(Debug, Clone)]
pub struct CountryItem {
    code: &'static str,
    title: SharedString,
}

impl SearchableListItem for CountryItem {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.code
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        self.title.to_lowercase().contains(&q) || self.code.eq_ignore_ascii_case(&q)
    }
}

pub type CountryPicker = Entity<ComboboxState<SearchableVec<CountryItem>>>;

/// The picker, kept in sync with the saved countries both ways.
pub fn country_picker<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
) -> (CountryPicker, Subscription) {
    let items: Vec<CountryItem> = holidays::countries()
        .iter()
        .map(|c| CountryItem {
            code: c.code,
            title: c.name.into(),
        })
        .collect();
    let chosen = crate::holidays::chosen(cx);
    let picker = cx.new(|cx| {
        let mut state = ComboboxState::new(SearchableVec::new(items), vec![], window, cx)
            .multiple(true)
            .searchable(true);
        state.set_selected_values(&chosen, window, cx);
        state
    });
    let sub = cx.subscribe(
        &picker,
        |_, _, event: &ComboboxEvent<SearchableVec<CountryItem>>, cx| {
            let (ComboboxEvent::Change(codes) | ComboboxEvent::Confirm(codes)) = event;
            let codes: Vec<String> = codes.iter().map(|c| (*c).to_owned()).collect();
            appearance::update(cx, |s| s.holiday_countries = Some(codes));
        },
    );
    (picker, sub)
}

/// Brings the picker back in line after a change made elsewhere.
pub fn sync_picker(picker: &CountryPicker, window: &mut Window, cx: &mut App) {
    let chosen = crate::holidays::chosen(cx);
    if picker.read(cx).selected_values() != chosen {
        picker.update(cx, |p, cx| p.set_selected_values(&chosen, window, cx));
    }
}

pub fn holiday_group(picker: CountryPicker) -> SettingGroup {
    let show = SettingItem::new(
        "Show holidays",
        SettingField::switch(
            |cx| appearance::settings(cx).holidays,
            |on, cx| appearance::update(cx, |s| s.holidays = on),
        ),
    )
    .description("Public holidays appear as a quiet line under the date, never as events.")
    .keywords([
        "holiday",
        "holidays",
        "public holiday",
        "calendar",
        "festival",
    ]);
    let countries = SettingItem::new(
        "Countries",
        SettingField::render(move |_, _, _| {
            Combobox::new(&picker)
                .placeholder("Choose countries")
                .search_placeholder("Search countries")
                .menu_width(px(280.))
                .w(rems(16.))
                .into_any_element()
        }),
    )
    .description({
        let (first, last) = holidays::years();
        format!(
            "Pick one or more. Holiday dates are built in for {first}–{last}, from official calendars."
        )
    })
    .keywords(["country", "countries", "india", "region", "holiday"]);
    let observances = SettingItem::new(
        "Festivals and observances",
        SettingField::switch(
            |cx| appearance::settings(cx).holiday_observances,
            |on, cx| appearance::update(cx, |s| s.holiday_observances = on),
        ),
    )
    .description("Days that aren't days off everywhere, like Holi, Raksha Bandhan or Mother's Day.")
    .keywords([
        "festival",
        "observance",
        "optional holiday",
        "important days",
    ]);
    let here = SettingItem::new(
        "Holidays where you are",
        SettingField::switch(
            |cx| appearance::settings(cx).holidays_where_you_are,
            |on, cx| appearance::update(cx, |s| s.holidays_where_you_are = on),
        ),
    )
    .description(
        "When you travel, also show the holidays of the country you're in, labelled with it. \
         Uses your Mac's time zone; your location is never asked for or sent.",
    )
    .keywords(["travel", "abroad", "location", "time zone", "holiday"]);
    SettingGroup::new()
        .title("Holidays")
        .items(vec![show, countries, observances, here])
}
