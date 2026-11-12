//! Her standing settings (`doom_policy`; the SDK plan's stage 7, `docs/POLICY_SURVEY.md`): how
//! she fights (`stance`), what ammo she keeps for bosses (`save`), how readily she takes a
//! floor that hurts (`risk`), when the big powerups are taken (`powerups`). Each is numbers over
//! the run's own tuning, applied as her standing numbers; a stance taken for a while with
//! `doom_fight` goes on top of them, and ends with any change here.

use serde_json::{json, Map, Value};

/// How she fights, as numbers over the tuning (None: not a stance).
pub fn stance_tuning(name: &str) -> Option<Value> {
    Some(match name {
        // (careful is the default since 2026-11-11, the user's: its numbers are the tuning's
        // own, given all the same, so that over another standing stance, for a while, it's
        // careful; steady the old middle)
        "careful" => json!({ "engage": { "stand_threat": 9.0, "low_health": 50.0 },
            "retreat": { "health": 55.0 }, "guns": { "outmatched": 20.0 } }),
        "steady" => json!({ "engage": { "stand_threat": 6.0, "low_health": 35.0 },
            "retreat": { "health": 40.0 }, "guns": { "outmatched": 30.0 } }),
        "bold" => json!({ "engage": { "stand_threat": 3.0, "low_health": 25.0 },
            "retreat": { "health": 30.0 }, "guns": { "outmatched": 45.0 } }),
        "avoid" => json!({ "engage": { "stand_threat": 9.0, "low_health": 50.0,
            "keep_clear": 1536.0, "leave_be": 8.0 }, "retreat": { "health": 55.0 },
            "guns": { "outmatched": 20.0 }, "errands": { "danger_near": 1024.0 } }),
        // (everything fought, head on, never fallen back from: dodging stays, a reflex)
        "yolo" => json!({ "engage": { "stand_threat": 0.0, "low_health": 0.0,
            "leave_be": 1000.0 }, "retreat": { "health": 0.0 },
            "guns": { "outmatched": 1000.0 } }),
        _ => return None,
    })
}

/// Her settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub stance: &'static str,
    /// the guns kept for bosses: "cells" (the plasma rifle and the BFG), "none"
    pub save: &'static str,
    /// floors that hurt: "careful", "normal", "bold"
    pub risk: &'static str,
    /// soulspheres, megaspheres, invulnerability: "need" (when they'd add enough, or a fight
    /// needs it), "now" (as they come)
    pub powerups: &'static str,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            stance: "careful",
            save: "cells",
            risk: "normal",
            powerups: "need",
        }
    }
}

/// One of a setting's values, by name (None: not one of them).
pub fn value(setting: &str, v: &str) -> Option<&'static str> {
    let v = v.trim().to_lowercase();
    let of: &[&'static str] = match setting {
        "stance" => &["careful", "steady", "bold", "avoid", "yolo"],
        "save" => &["cells", "none"],
        "risk" => &["careful", "normal", "bold"],
        "powerups" => &["need", "now"],
        _ => return None,
    };
    of.iter().copied().find(|o| *o == v)
}

/// Each value's line in the schema: what it is, and what it cost in the score (the user's: the
/// cost told, the choice hers).
pub fn help() -> &'static str {
    SETTINGS_HELP
}

pub const SETTINGS_HELP: &str = "stance: careful (the default: backs off sooner); steady (stands and fights more); bold (fights more still, finishes fewer levels); avoid (keeps clear of fights: to get past one, not to leave on); yolo (fights everything head on; asked first). save: cells (plasma and BFG kept for bosses), none. risk: careful, normal, bold (floors that hurt). powerups: need (spheres and invulnerability when they'd help), now (as they come).";

impl Policy {
    /// The numbers over the run's tuning these settings come to.
    pub fn tuning(&self) -> Value {
        let mut all: Map<String, Value> = Map::new();
        let mut put = |over: Value| {
            for (group, fields) in over.as_object().into_iter().flatten() {
                let g = all
                    .entry(group.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let (Some(g), Some(fields)) = (g.as_object_mut(), fields.as_object()) {
                    for (k, v) in fields {
                        g.insert(k.clone(), v.clone());
                    }
                }
            }
        };
        put(stance_tuning(self.stance).unwrap_or_else(|| json!({})));
        if self.save == "none" {
            put(json!({ "guns": { "save_from": 8 } }));
        }
        match self.risk {
            "careful" => put(json!({ "errands": { "floor_cost": 40.0 } })),
            "bold" => put(json!({ "errands": { "floor_cost": 10.0 } })),
            _ => {}
        }
        if self.powerups == "now" {
            put(json!({ "errands": { "mega_gain": 1.0, "soul_gain": 1.0,
                "invulnerability_now": true } }));
        }
        Value::Object(all)
    }

    /// In a line, as she's told it.
    pub fn line(&self) -> String {
        format!(
            "stance {}, save {}, risk {}, powerups {}",
            self.stance, self.save, self.risk, self.powerups
        )
    }

    /// What isn't at its default, in a line ("" if all are).
    pub fn changed(&self) -> String {
        let d = Policy::default();
        let mut out = Vec::new();
        for (name, now, was) in [
            ("stance", self.stance, d.stance),
            ("save", self.save, d.save),
            ("risk", self.risk, d.risk),
            ("powerups", self.powerups, d.powerups),
        ] {
            if now != was {
                out.push(format!("{name} {now}"));
            }
        }
        out.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::{stance_tuning, value, Policy};
    use crate::tactical::tuning::Tuning;

    /// Every setting's numbers are ones the tuning has.
    #[test]
    fn every_setting_applies() {
        for stance in ["steady", "bold", "careful", "avoid", "yolo"] {
            for (save, risk, powerups) in [
                ("cells", "normal", "need"),
                ("none", "bold", "now"),
                ("cells", "careful", "need"),
            ] {
                let p = Policy {
                    stance,
                    save,
                    risk,
                    powerups,
                };
                Tuning::default()
                    .with(&p.tuning())
                    .unwrap_or_else(|e| panic!("{p:?}: {e}"));
            }
            assert!(stance_tuning(stance).is_some());
        }
        assert_eq!(value("stance", "YOLO"), Some("yolo"));
        assert_eq!(value("save", "rockets"), None);
        assert_eq!(Policy::default().changed(), "");
    }
}
