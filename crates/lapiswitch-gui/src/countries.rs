//! LTBox country names and flag-icons assets (MIT). Choices are limited to
//! the same canonical 41 codes accepted by LTBox's country patcher.
#[derive(Debug, Clone, Copy)]
pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
    pub korean: &'static str,
    pub chinese: &'static str,
    pub japanese: &'static str,
    pub russian: &'static str,
}
impl Country {
    /// The name shown under the English name. Korean and English keep the
    /// Korean name as before; the other languages show their own.
    pub fn local_name(&self, language: crate::settings::Language) -> &'static str {
        use crate::settings::Language::*;
        match language {
            Korean | English => self.korean,
            Chinese => self.chinese,
            Japanese => self.japanese,
            Russian => self.russian,
        }
    }
}
pub const COUNTRIES: &[Country] = &[
    Country {
        code: "AE",
        name: "United Arab Emirates",
        korean: "아랍에미리트",
        chinese: "阿联酋",
        japanese: "アラブ首長国連邦",
        russian: "ОАЭ",
    },
    Country {
        code: "AT",
        name: "Austria",
        korean: "오스트리아",
        chinese: "奥地利",
        japanese: "オーストリア",
        russian: "Австрия",
    },
    Country {
        code: "AU",
        name: "Australia",
        korean: "호주",
        chinese: "澳大利亚",
        japanese: "オーストラリア",
        russian: "Австралия",
    },
    Country {
        code: "BE",
        name: "Belgium",
        korean: "벨기에",
        chinese: "比利时",
        japanese: "ベルギー",
        russian: "Бельгия",
    },
    Country {
        code: "BG",
        name: "Bulgaria",
        korean: "불가리아",
        chinese: "保加利亚",
        japanese: "ブルガリア",
        russian: "Болгария",
    },
    Country {
        code: "BR",
        name: "Brazil",
        korean: "브라질",
        chinese: "巴西",
        japanese: "ブラジル",
        russian: "Бразилия",
    },
    Country {
        code: "CA",
        name: "Canada",
        korean: "캐나다",
        chinese: "加拿大",
        japanese: "カナダ",
        russian: "Канада",
    },
    Country {
        code: "CN",
        name: "China",
        korean: "중국",
        chinese: "中国",
        japanese: "中国",
        russian: "Китай",
    },
    Country {
        code: "CY",
        name: "Cyprus",
        korean: "키프로스",
        chinese: "塞浦路斯",
        japanese: "キプロス",
        russian: "Кипр",
    },
    Country {
        code: "CZ",
        name: "Czech Republic",
        korean: "체코",
        chinese: "捷克",
        japanese: "チェコ",
        russian: "Чехия",
    },
    Country {
        code: "DE",
        name: "Germany",
        korean: "독일",
        chinese: "德国",
        japanese: "ドイツ",
        russian: "Германия",
    },
    Country {
        code: "DK",
        name: "Denmark",
        korean: "덴마크",
        chinese: "丹麦",
        japanese: "デンマーク",
        russian: "Дания",
    },
    Country {
        code: "EE",
        name: "Estonia",
        korean: "에스토니아",
        chinese: "爱沙尼亚",
        japanese: "エストニア",
        russian: "Эстония",
    },
    Country {
        code: "ES",
        name: "Spain",
        korean: "스페인",
        chinese: "西班牙",
        japanese: "スペイン",
        russian: "Испания",
    },
    Country {
        code: "FI",
        name: "Finland",
        korean: "핀란드",
        chinese: "芬兰",
        japanese: "フィンランド",
        russian: "Финляндия",
    },
    Country {
        code: "FR",
        name: "France",
        korean: "프랑스",
        chinese: "法国",
        japanese: "フランス",
        russian: "Франция",
    },
    Country {
        code: "GB",
        name: "United Kingdom",
        korean: "영국",
        chinese: "英国",
        japanese: "イギリス",
        russian: "Великобритания",
    },
    Country {
        code: "GR",
        name: "Greece",
        korean: "그리스",
        chinese: "希腊",
        japanese: "ギリシャ",
        russian: "Греция",
    },
    Country {
        code: "HR",
        name: "Croatia",
        korean: "크로아티아",
        chinese: "克罗地亚",
        japanese: "クロアチア",
        russian: "Хорватия",
    },
    Country {
        code: "HU",
        name: "Hungary",
        korean: "헝가리",
        chinese: "匈牙利",
        japanese: "ハンガリー",
        russian: "Венгрия",
    },
    Country {
        code: "IE",
        name: "Ireland",
        korean: "아일랜드",
        chinese: "爱尔兰",
        japanese: "アイルランド",
        russian: "Ирландия",
    },
    Country {
        code: "IN",
        name: "India",
        korean: "인도",
        chinese: "印度",
        japanese: "インド",
        russian: "Индия",
    },
    Country {
        code: "IT",
        name: "Italy",
        korean: "이탈리아",
        chinese: "意大利",
        japanese: "イタリア",
        russian: "Италия",
    },
    Country {
        code: "JP",
        name: "Japan",
        korean: "일본",
        chinese: "日本",
        japanese: "日本",
        russian: "Япония",
    },
    Country {
        code: "KR",
        name: "Korea",
        korean: "한국 대한민국",
        chinese: "韩国",
        japanese: "韓国",
        russian: "Южная Корея",
    },
    Country {
        code: "LT",
        name: "Lithuania",
        korean: "리투아니아",
        chinese: "立陶宛",
        japanese: "リトアニア",
        russian: "Литва",
    },
    Country {
        code: "LU",
        name: "Luxembourg",
        korean: "룩셈부르크",
        chinese: "卢森堡",
        japanese: "ルクセンブルク",
        russian: "Люксембург",
    },
    Country {
        code: "LV",
        name: "Latvia",
        korean: "라트비아",
        chinese: "拉脱维亚",
        japanese: "ラトビア",
        russian: "Латвия",
    },
    Country {
        code: "MT",
        name: "Malta",
        korean: "몰타",
        chinese: "马耳他",
        japanese: "マルタ",
        russian: "Мальта",
    },
    Country {
        code: "MX",
        name: "Mexico",
        korean: "멕시코",
        chinese: "墨西哥",
        japanese: "メキシコ",
        russian: "Мексика",
    },
    Country {
        code: "NL",
        name: "Netherlands",
        korean: "네덜란드",
        chinese: "荷兰",
        japanese: "オランダ",
        russian: "Нидерланды",
    },
    Country {
        code: "PL",
        name: "Poland",
        korean: "폴란드",
        chinese: "波兰",
        japanese: "ポーランド",
        russian: "Польша",
    },
    Country {
        code: "PT",
        name: "Portugal",
        korean: "포르투갈",
        chinese: "葡萄牙",
        japanese: "ポルトガル",
        russian: "Португалия",
    },
    Country {
        code: "RO",
        name: "Romania",
        korean: "루마니아",
        chinese: "罗马尼亚",
        japanese: "ルーマニア",
        russian: "Румыния",
    },
    Country {
        code: "RU",
        name: "Russia",
        korean: "러시아",
        chinese: "俄罗斯",
        japanese: "ロシア",
        russian: "Россия",
    },
    Country {
        code: "SA",
        name: "Saudi Arabia",
        korean: "사우디아라비아",
        chinese: "沙特阿拉伯",
        japanese: "サウジアラビア",
        russian: "Саудовская Аравия",
    },
    Country {
        code: "SE",
        name: "Sweden",
        korean: "스웨덴",
        chinese: "瑞典",
        japanese: "スウェーデン",
        russian: "Швеция",
    },
    Country {
        code: "SI",
        name: "Slovenia",
        korean: "슬로베니아",
        chinese: "斯洛文尼亚",
        japanese: "スロベニア",
        russian: "Словения",
    },
    Country {
        code: "SK",
        name: "Slovakia",
        korean: "슬로바키아",
        chinese: "斯洛伐克",
        japanese: "スロバキア",
        russian: "Словакия",
    },
    Country {
        code: "US",
        name: "United States",
        korean: "미국",
        chinese: "美国",
        japanese: "アメリカ合衆国",
        russian: "США",
    },
    Country {
        code: "WW",
        name: "Worldwide",
        korean: "전세계 글로벌",
        chinese: "全球",
        japanese: "全世界",
        russian: "Весь мир",
    },
];
pub fn find(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code == code)
}
pub fn search(query: &str) -> impl Iterator<Item = &'static Country> {
    let query = query.trim().to_lowercase();
    COUNTRIES.iter().filter(move |c| {
        c.code.to_lowercase().contains(&query)
            || c.name.to_lowercase().contains(&query)
            || c.korean.contains(&query)
            || c.chinese.contains(&query)
            || c.japanese.contains(&query)
            || c.russian.to_lowercase().contains(&query)
    })
}
pub fn flag(code: &str) -> iced::widget::svg::Handle {
    iced::widget::svg::Handle::from_memory(match code {
        "AE" => include_bytes!("../assets/flags/ae.svg").as_slice(),
        "AT" => include_bytes!("../assets/flags/at.svg").as_slice(),
        "AU" => include_bytes!("../assets/flags/au.svg").as_slice(),
        "BE" => include_bytes!("../assets/flags/be.svg").as_slice(),
        "BG" => include_bytes!("../assets/flags/bg.svg").as_slice(),
        "BR" => include_bytes!("../assets/flags/br.svg").as_slice(),
        "CA" => include_bytes!("../assets/flags/ca.svg").as_slice(),
        "CN" => include_bytes!("../assets/flags/cn.svg").as_slice(),
        "CY" => include_bytes!("../assets/flags/cy.svg").as_slice(),
        "CZ" => include_bytes!("../assets/flags/cz.svg").as_slice(),
        "DE" => include_bytes!("../assets/flags/de.svg").as_slice(),
        "DK" => include_bytes!("../assets/flags/dk.svg").as_slice(),
        "EE" => include_bytes!("../assets/flags/ee.svg").as_slice(),
        "ES" => include_bytes!("../assets/flags/es.svg").as_slice(),
        "FI" => include_bytes!("../assets/flags/fi.svg").as_slice(),
        "FR" => include_bytes!("../assets/flags/fr.svg").as_slice(),
        "GB" => include_bytes!("../assets/flags/gb.svg").as_slice(),
        "GR" => include_bytes!("../assets/flags/gr.svg").as_slice(),
        "HR" => include_bytes!("../assets/flags/hr.svg").as_slice(),
        "HU" => include_bytes!("../assets/flags/hu.svg").as_slice(),
        "IE" => include_bytes!("../assets/flags/ie.svg").as_slice(),
        "IN" => include_bytes!("../assets/flags/in.svg").as_slice(),
        "IT" => include_bytes!("../assets/flags/it.svg").as_slice(),
        "JP" => include_bytes!("../assets/flags/jp.svg").as_slice(),
        "KR" => include_bytes!("../assets/flags/kr.svg").as_slice(),
        "LT" => include_bytes!("../assets/flags/lt.svg").as_slice(),
        "LU" => include_bytes!("../assets/flags/lu.svg").as_slice(),
        "LV" => include_bytes!("../assets/flags/lv.svg").as_slice(),
        "MT" => include_bytes!("../assets/flags/mt.svg").as_slice(),
        "MX" => include_bytes!("../assets/flags/mx.svg").as_slice(),
        "NL" => include_bytes!("../assets/flags/nl.svg").as_slice(),
        "PL" => include_bytes!("../assets/flags/pl.svg").as_slice(),
        "PT" => include_bytes!("../assets/flags/pt.svg").as_slice(),
        "RO" => include_bytes!("../assets/flags/ro.svg").as_slice(),
        "RU" => include_bytes!("../assets/flags/ru.svg").as_slice(),
        "SA" => include_bytes!("../assets/flags/sa.svg").as_slice(),
        "SE" => include_bytes!("../assets/flags/se.svg").as_slice(),
        "SI" => include_bytes!("../assets/flags/si.svg").as_slice(),
        "SK" => include_bytes!("../assets/flags/sk.svg").as_slice(),
        "US" => include_bytes!("../assets/flags/us.svg").as_slice(),
        "WW" => include_bytes!("../assets/flags/ww.svg").as_slice(),
        _ => include_bytes!("../assets/flags/ww.svg").as_slice(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_accepts_code_name_and_korean() {
        for q in [
            " kr ",
            "korea",
            "한국",
            "韩国",
            "韓国",
            "южная корея",
            "Корея",
        ] {
            assert_eq!(search(q).next().unwrap().code, "KR");
        }
        assert_eq!(search("zzzz").count(), 0);
    }
    #[test]
    fn choices_match_the_patcher_and_have_flags() {
        assert_eq!(COUNTRIES.len(), lapiswitch_core::country::KNOWN_CODES.len());
        for code in lapiswitch_core::country::KNOWN_CODES {
            assert!(find(code).is_some());
            let _ = flag(code);
        }
    }
}
