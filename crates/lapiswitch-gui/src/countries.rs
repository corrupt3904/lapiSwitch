//! LTBox country names and flag-icons assets (MIT). The choices are the 89
//! codes of LTBox's country picker (`ltbox-gui` `COUNTRY_CODES`).
#[derive(Debug, Clone, Copy)]
pub struct Country {
    pub code: &'static str,
    /// The English name; every language's name is `country_<code>` in
    /// `lang/<code>.json`.
    pub name: &'static str,
}
impl Country {
    /// The name shown under the English name. Korean and English keep the
    /// Korean name as before; the other languages show their own.
    pub fn local_name(&self, language: crate::settings::Language) -> &'static str {
        use crate::settings::Language::*;
        let language = if language == English {
            Korean
        } else {
            language
        };
        language.country_name(self.code)
    }
}
pub const COUNTRIES: &[Country] = &[
    Country {
        code: "AE",
        name: "United Arab Emirates",
    },
    Country {
        code: "AM",
        name: "Armenia",
    },
    Country {
        code: "AR",
        name: "Argentina",
    },
    Country {
        code: "AT",
        name: "Austria",
    },
    Country {
        code: "AU",
        name: "Australia",
    },
    Country {
        code: "AZ",
        name: "Azerbaijan",
    },
    Country {
        code: "BE",
        name: "Belgium",
    },
    Country {
        code: "BG",
        name: "Bulgaria",
    },
    Country {
        code: "BH",
        name: "Bahrain",
    },
    Country {
        code: "BR",
        name: "Brazil",
    },
    Country {
        code: "CA",
        name: "Canada",
    },
    Country {
        code: "CH",
        name: "Switzerland",
    },
    Country {
        code: "CL",
        name: "Chile",
    },
    Country {
        code: "CN",
        name: "China",
    },
    Country {
        code: "CO",
        name: "Colombia",
    },
    Country {
        code: "CR",
        name: "Costa Rica",
    },
    Country {
        code: "CY",
        name: "Cyprus",
    },
    Country {
        code: "CZ",
        name: "Czech Republic",
    },
    Country {
        code: "DE",
        name: "Germany",
    },
    Country {
        code: "DK",
        name: "Denmark",
    },
    Country {
        code: "EC",
        name: "Ecuador",
    },
    Country {
        code: "EE",
        name: "Estonia",
    },
    Country {
        code: "EG",
        name: "Egypt",
    },
    Country {
        code: "ES",
        name: "Spain",
    },
    Country {
        code: "FI",
        name: "Finland",
    },
    Country {
        code: "FR",
        name: "France",
    },
    Country {
        code: "GB",
        name: "United Kingdom",
    },
    Country {
        code: "GE",
        name: "Georgia",
    },
    Country {
        code: "GH",
        name: "Ghana",
    },
    Country {
        code: "GR",
        name: "Greece",
    },
    Country {
        code: "GT",
        name: "Guatemala",
    },
    Country {
        code: "HK",
        name: "Hong Kong",
    },
    Country {
        code: "HR",
        name: "Croatia",
    },
    Country {
        code: "HU",
        name: "Hungary",
    },
    Country {
        code: "ID",
        name: "Indonesia",
    },
    Country {
        code: "IL",
        name: "Israel",
    },
    Country {
        code: "IN",
        name: "India",
    },
    Country {
        code: "IS",
        name: "Iceland",
    },
    Country {
        code: "IT",
        name: "Italy",
    },
    Country {
        code: "JO",
        name: "Jordan",
    },
    Country {
        code: "JP",
        name: "Japan",
    },
    Country {
        code: "KE",
        name: "Kenya",
    },
    Country {
        code: "KG",
        name: "Kyrgyzstan",
    },
    Country {
        code: "KR",
        name: "Korea",
    },
    Country {
        code: "KW",
        name: "Kuwait",
    },
    Country {
        code: "KZ",
        name: "Kazakhstan",
    },
    Country {
        code: "LB",
        name: "Lebanon",
    },
    Country {
        code: "LT",
        name: "Lithuania",
    },
    Country {
        code: "LV",
        name: "Latvia",
    },
    Country {
        code: "MA",
        name: "Morocco",
    },
    Country {
        code: "MD",
        name: "Moldova",
    },
    Country {
        code: "MX",
        name: "Mexico",
    },
    Country {
        code: "MY",
        name: "Malaysia",
    },
    Country {
        code: "MZ",
        name: "Mozambique",
    },
    Country {
        code: "NG",
        name: "Nigeria",
    },
    Country {
        code: "NL",
        name: "Netherlands",
    },
    Country {
        code: "NO",
        name: "Norway",
    },
    Country {
        code: "NZ",
        name: "New Zealand",
    },
    Country {
        code: "OM",
        name: "Oman",
    },
    Country {
        code: "PA",
        name: "Panama",
    },
    Country {
        code: "PE",
        name: "Peru",
    },
    Country {
        code: "PH",
        name: "Philippines",
    },
    Country {
        code: "PK",
        name: "Pakistan",
    },
    Country {
        code: "PL",
        name: "Poland",
    },
    Country {
        code: "PT",
        name: "Portugal",
    },
    Country {
        code: "QA",
        name: "Qatar",
    },
    Country {
        code: "RO",
        name: "Romania",
    },
    Country {
        code: "RS",
        name: "Serbia",
    },
    Country {
        code: "RU",
        name: "Russia",
    },
    Country {
        code: "SA",
        name: "Saudi Arabia",
    },
    Country {
        code: "SE",
        name: "Sweden",
    },
    Country {
        code: "SG",
        name: "Singapore",
    },
    Country {
        code: "SI",
        name: "Slovenia",
    },
    Country {
        code: "SK",
        name: "Slovakia",
    },
    Country {
        code: "SV",
        name: "El Salvador",
    },
    Country {
        code: "TH",
        name: "Thailand",
    },
    Country {
        code: "TJ",
        name: "Tajikistan",
    },
    Country {
        code: "TN",
        name: "Tunisia",
    },
    Country {
        code: "TR",
        name: "Turkey",
    },
    Country {
        code: "TW",
        name: "Taiwan",
    },
    Country {
        code: "TZ",
        name: "Tanzania",
    },
    Country {
        code: "UA",
        name: "Ukraine",
    },
    Country {
        code: "UG",
        name: "Uganda",
    },
    Country {
        code: "US",
        name: "United States",
    },
    Country {
        code: "UY",
        name: "Uruguay",
    },
    Country {
        code: "UZ",
        name: "Uzbekistan",
    },
    Country {
        code: "VE",
        name: "Venezuela",
    },
    Country {
        code: "VN",
        name: "Vietnam",
    },
    Country {
        code: "ZA",
        name: "South Africa",
    },
];
pub fn find(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code == code)
}
pub fn search(query: &str) -> impl Iterator<Item = &'static Country> {
    let query = query.trim().to_lowercase();
    COUNTRIES.iter().filter(move |c| {
        c.code.to_lowercase().contains(&query)
            || crate::settings::Language::ALL.iter().any(|language| {
                language
                    .country_name(c.code)
                    .to_lowercase()
                    .contains(&query)
            })
    })
}
pub fn flag(code: &str) -> iced::widget::svg::Handle {
    iced::widget::svg::Handle::from_memory(match code {
        "AE" => include_bytes!("../assets/flags/ae.svg").as_slice(),
        "AM" => include_bytes!("../assets/flags/am.svg").as_slice(),
        "AR" => include_bytes!("../assets/flags/ar.svg").as_slice(),
        "AT" => include_bytes!("../assets/flags/at.svg").as_slice(),
        "AU" => include_bytes!("../assets/flags/au.svg").as_slice(),
        "AZ" => include_bytes!("../assets/flags/az.svg").as_slice(),
        "BE" => include_bytes!("../assets/flags/be.svg").as_slice(),
        "BG" => include_bytes!("../assets/flags/bg.svg").as_slice(),
        "BH" => include_bytes!("../assets/flags/bh.svg").as_slice(),
        "BR" => include_bytes!("../assets/flags/br.svg").as_slice(),
        "CA" => include_bytes!("../assets/flags/ca.svg").as_slice(),
        "CH" => include_bytes!("../assets/flags/ch.svg").as_slice(),
        "CL" => include_bytes!("../assets/flags/cl.svg").as_slice(),
        "CN" => include_bytes!("../assets/flags/cn.svg").as_slice(),
        "CO" => include_bytes!("../assets/flags/co.svg").as_slice(),
        "CR" => include_bytes!("../assets/flags/cr.svg").as_slice(),
        "CY" => include_bytes!("../assets/flags/cy.svg").as_slice(),
        "CZ" => include_bytes!("../assets/flags/cz.svg").as_slice(),
        "DE" => include_bytes!("../assets/flags/de.svg").as_slice(),
        "DK" => include_bytes!("../assets/flags/dk.svg").as_slice(),
        "EC" => include_bytes!("../assets/flags/ec.svg").as_slice(),
        "EE" => include_bytes!("../assets/flags/ee.svg").as_slice(),
        "EG" => include_bytes!("../assets/flags/eg.svg").as_slice(),
        "ES" => include_bytes!("../assets/flags/es.svg").as_slice(),
        "FI" => include_bytes!("../assets/flags/fi.svg").as_slice(),
        "FR" => include_bytes!("../assets/flags/fr.svg").as_slice(),
        "GB" => include_bytes!("../assets/flags/gb.svg").as_slice(),
        "GE" => include_bytes!("../assets/flags/ge.svg").as_slice(),
        "GH" => include_bytes!("../assets/flags/gh.svg").as_slice(),
        "GR" => include_bytes!("../assets/flags/gr.svg").as_slice(),
        "GT" => include_bytes!("../assets/flags/gt.svg").as_slice(),
        "HK" => include_bytes!("../assets/flags/hk.svg").as_slice(),
        "HR" => include_bytes!("../assets/flags/hr.svg").as_slice(),
        "HU" => include_bytes!("../assets/flags/hu.svg").as_slice(),
        "ID" => include_bytes!("../assets/flags/id.svg").as_slice(),
        "IL" => include_bytes!("../assets/flags/il.svg").as_slice(),
        "IN" => include_bytes!("../assets/flags/in.svg").as_slice(),
        "IS" => include_bytes!("../assets/flags/is.svg").as_slice(),
        "IT" => include_bytes!("../assets/flags/it.svg").as_slice(),
        "JO" => include_bytes!("../assets/flags/jo.svg").as_slice(),
        "JP" => include_bytes!("../assets/flags/jp.svg").as_slice(),
        "KE" => include_bytes!("../assets/flags/ke.svg").as_slice(),
        "KG" => include_bytes!("../assets/flags/kg.svg").as_slice(),
        "KR" => include_bytes!("../assets/flags/kr.svg").as_slice(),
        "KW" => include_bytes!("../assets/flags/kw.svg").as_slice(),
        "KZ" => include_bytes!("../assets/flags/kz.svg").as_slice(),
        "LB" => include_bytes!("../assets/flags/lb.svg").as_slice(),
        "LT" => include_bytes!("../assets/flags/lt.svg").as_slice(),
        "LV" => include_bytes!("../assets/flags/lv.svg").as_slice(),
        "MA" => include_bytes!("../assets/flags/ma.svg").as_slice(),
        "MD" => include_bytes!("../assets/flags/md.svg").as_slice(),
        "MX" => include_bytes!("../assets/flags/mx.svg").as_slice(),
        "MY" => include_bytes!("../assets/flags/my.svg").as_slice(),
        "MZ" => include_bytes!("../assets/flags/mz.svg").as_slice(),
        "NG" => include_bytes!("../assets/flags/ng.svg").as_slice(),
        "NL" => include_bytes!("../assets/flags/nl.svg").as_slice(),
        "NO" => include_bytes!("../assets/flags/no.svg").as_slice(),
        "NZ" => include_bytes!("../assets/flags/nz.svg").as_slice(),
        "OM" => include_bytes!("../assets/flags/om.svg").as_slice(),
        "PA" => include_bytes!("../assets/flags/pa.svg").as_slice(),
        "PE" => include_bytes!("../assets/flags/pe.svg").as_slice(),
        "PH" => include_bytes!("../assets/flags/ph.svg").as_slice(),
        "PK" => include_bytes!("../assets/flags/pk.svg").as_slice(),
        "PL" => include_bytes!("../assets/flags/pl.svg").as_slice(),
        "PT" => include_bytes!("../assets/flags/pt.svg").as_slice(),
        "QA" => include_bytes!("../assets/flags/qa.svg").as_slice(),
        "RO" => include_bytes!("../assets/flags/ro.svg").as_slice(),
        "RS" => include_bytes!("../assets/flags/rs.svg").as_slice(),
        "RU" => include_bytes!("../assets/flags/ru.svg").as_slice(),
        "SA" => include_bytes!("../assets/flags/sa.svg").as_slice(),
        "SE" => include_bytes!("../assets/flags/se.svg").as_slice(),
        "SG" => include_bytes!("../assets/flags/sg.svg").as_slice(),
        "SI" => include_bytes!("../assets/flags/si.svg").as_slice(),
        "SK" => include_bytes!("../assets/flags/sk.svg").as_slice(),
        "SV" => include_bytes!("../assets/flags/sv.svg").as_slice(),
        "TH" => include_bytes!("../assets/flags/th.svg").as_slice(),
        "TJ" => include_bytes!("../assets/flags/tj.svg").as_slice(),
        "TN" => include_bytes!("../assets/flags/tn.svg").as_slice(),
        "TR" => include_bytes!("../assets/flags/tr.svg").as_slice(),
        "TW" => include_bytes!("../assets/flags/tw.svg").as_slice(),
        "TZ" => include_bytes!("../assets/flags/tz.svg").as_slice(),
        "UA" => include_bytes!("../assets/flags/ua.svg").as_slice(),
        "UG" => include_bytes!("../assets/flags/ug.svg").as_slice(),
        "US" => include_bytes!("../assets/flags/us.svg").as_slice(),
        "UY" => include_bytes!("../assets/flags/uy.svg").as_slice(),
        "UZ" => include_bytes!("../assets/flags/uz.svg").as_slice(),
        "VE" => include_bytes!("../assets/flags/ve.svg").as_slice(),
        "VN" => include_bytes!("../assets/flags/vn.svg").as_slice(),
        "ZA" => include_bytes!("../assets/flags/za.svg").as_slice(),
        _ => include_bytes!("../assets/flags/ww.svg").as_slice(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Language;
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
        assert_eq!(
            COUNTRIES.len(),
            lapiswitch_core::country::CHOICE_CODES.len()
        );
        for code in lapiswitch_core::country::CHOICE_CODES {
            let country = find(code).unwrap();
            let _ = flag(code);
            assert_eq!(Language::English.country_name(code), country.name);
            for language in Language::ALL {
                assert_ne!(language.country_name(code), *code, "{language:?}");
            }
        }
    }
}
