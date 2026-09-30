use crate::cores::{all_cores, CoreInfo};

pub struct Notice {
    pub name: &'static str,
    pub license: &'static str,
    pub url: &'static str,
}

const fn notice(name: &'static str, license: &'static str, url: &'static str) -> Notice {
    Notice { name, license, url }
}

const ROMP: Notice = notice(
    "Romp",
    "GPL-3.0 or later",
    "https://github.com/RompEmu/RompEmu/blob/main/LICENSE",
);
const SLINT: Notice = notice(
    "Slint",
    "GPL-3.0",
    "https://github.com/slint-ui/slint/blob/master/LICENSES/GPL-3.0-only.txt",
);
const XEMU: Notice = notice(
    "xemu",
    "GPL-2.0",
    "https://github.com/xemu-project/xemu/blob/master/LICENSE",
);
const RPCS3: Notice = notice(
    "RPCS3",
    "GPL-2.0",
    "https://github.com/RPCS3/rpcs3/blob/master/LICENSE",
);
const RCHEEVOS: Notice = notice(
    "rcheevos",
    "MIT",
    "https://github.com/RetroAchievements/rcheevos/blob/develop/LICENSE",
);
const MOLTENVK: Notice = notice(
    "MoltenVK",
    "Apache-2.0",
    "https://github.com/KhronosGroup/MoltenVK/blob/main/LICENSE",
);

static CORE_LICENSES: &[(&str, &str, &str)] = &[
    (
        "armsx2",
        "GPL-3.0",
        "https://github.com/RompEmu/ARMSX2/blob/romp/COPYING.GPLv3",
    ),
    (
        "pcsx2",
        "GPL-3.0",
        "https://github.com/libretro/ps2/blob/master/COPYING.GPLv3",
    ),
    (
        "play",
        "BSD",
        "https://github.com/jpd002/Play-/blob/master/License.txt",
    ),
    (
        "snes9x",
        "Non-commercial",
        "https://github.com/libretro/snes9x/blob/master/LICENSE",
    ),
    (
        "nestopia",
        "GPL-2.0",
        "https://github.com/libretro/nestopia/blob/master/COPYING",
    ),
    (
        "genesis_plus_gx",
        "Non-commercial",
        "https://github.com/libretro/Genesis-Plus-GX/blob/master/LICENSE.txt",
    ),
    (
        "mgba",
        "MPL-2.0",
        "https://github.com/libretro/mgba/blob/master/LICENSE",
    ),
    (
        "flycast",
        "GPL-2.0",
        "https://github.com/flyinghead/flycast/blob/master/LICENSE",
    ),
    (
        "mupen64plus_next",
        "GPL-2.0",
        "https://github.com/libretro/mupen64plus-libretro-nx/blob/develop/LICENSE",
    ),
    (
        "mednafen_pce_fast",
        "GPL-2.0",
        "https://github.com/libretro/beetle-pce-fast-libretro/blob/master/COPYING",
    ),
    (
        "mednafen_pce",
        "GPL-2.0",
        "https://github.com/libretro/beetle-pce-libretro/blob/master/COPYING",
    ),
    (
        "mednafen_supergrafx",
        "GPL-2.0",
        "https://github.com/libretro/beetle-supergrafx-libretro/blob/master/COPYING",
    ),
    (
        "mednafen_psx_hw",
        "GPL-2.0",
        "https://github.com/libretro/beetle-psx-libretro/blob/master/COPYING",
    ),
    (
        "mednafen_saturn",
        "GPL-2.0",
        "https://github.com/libretro/beetle-saturn-libretro/blob/master/COPYING",
    ),
    (
        "stella",
        "GPL-2.0",
        "https://github.com/stella-emu/stella/blob/master/License.txt",
    ),
    (
        "mednafen_ngp",
        "GPL-2.0",
        "https://github.com/libretro/beetle-ngp-libretro/blob/master/COPYING",
    ),
    (
        "picodrive",
        "Non-commercial",
        "https://github.com/libretro/picodrive/blob/master/COPYING",
    ),
    (
        "virtualjaguar",
        "GPL-3.0",
        "https://github.com/libretro/virtualjaguar-libretro/blob/master/LICENSE",
    ),
    (
        "mednafen_wswan",
        "GPL-2.0",
        "https://github.com/libretro/beetle-wswan-libretro/blob/master/COPYING",
    ),
    (
        "mednafen_vb",
        "GPL-2.0",
        "https://github.com/libretro/beetle-vb-libretro/blob/master/COPYING",
    ),
    (
        "vecx",
        "GPL-3.0",
        "https://github.com/libretro/libretro-vecx/blob/master/LICENSE.md",
    ),
    (
        "vice_x64sc",
        "GPL-2.0",
        "https://github.com/libretro/vice-libretro/blob/master/COPYING",
    ),
    (
        "fuse",
        "GPL-3.0",
        "https://github.com/libretro/fuse-libretro/blob/master/LICENSE",
    ),
    (
        "puae",
        "GPL-2.0",
        "https://github.com/libretro/libretro-uae/blob/master/COPYING",
    ),
    (
        "mednafen_lynx",
        "GPL-2.0 and zlib",
        "https://github.com/libretro/beetle-lynx-libretro/blob/master/COPYING",
    ),
    (
        "freeintv",
        "GPL-2.0 or later",
        "https://github.com/libretro/FreeIntv/blob/master/LICENSE",
    ),
    (
        "o2em",
        "Artistic License",
        "https://github.com/libretro/libretro-o2em/blob/master/COPYING",
    ),
    (
        "ppsspp",
        "GPL-2.0 or later",
        "https://github.com/hrydgard/ppsspp/blob/master/LICENSE.TXT",
    ),
    (
        "opera",
        "LGPL, parts non-commercial",
        "https://github.com/libretro/opera-libretro",
    ),
    (
        "dosbox_pure",
        "GPL-2.0",
        "https://github.com/schellingb/dosbox-pure/blob/main/LICENSE",
    ),
    (
        "desmume",
        "GPL-2.0",
        "https://github.com/libretro/desmume/blob/master/license.txt",
    ),
    (
        "dolphin",
        "GPL-2.0 or later",
        "https://github.com/libretro/dolphin/blob/master/COPYING",
    ),
    (
        "bluemsx",
        "Mixed: BSD, GPL and freeware",
        "https://github.com/libretro/blueMSX-libretro/blob/master/license.txt",
    ),
    (
        "gearcoleco",
        "GPL-3.0",
        "https://github.com/drhelius/Gearcoleco/blob/main/LICENSE",
    ),
    (
        "same_cdi",
        "GPL-2.0 or later",
        "https://github.com/libretro/same_cdi/blob/master/COPYING",
    ),
    (
        "fbneo",
        "Non-commercial",
        "https://github.com/libretro/FBNeo/blob/master/LICENSE.md",
    ),
];

fn core_notice(core: &CoreInfo) -> Option<Notice> {
    CORE_LICENSES
        .iter()
        .find(|(id, _, _)| *id == core.id)
        .map(|(_, license, url)| notice(core.name, license, url))
}

/// Romp, then every emulator this computer can download, then the libraries it ships with.
pub fn notices() -> Vec<Notice> {
    let mut notices = vec![ROMP];
    let mut cores: Vec<&CoreInfo> = all_cores().collect();
    cores.sort_by_key(|c| c.name.to_lowercase());
    cores.dedup_by_key(|c| c.id);
    notices.extend(cores.into_iter().filter_map(core_notice));
    if crate::rpcs3::available() {
        notices.push(RPCS3);
    }
    if crate::xemu::available() {
        notices.push(XEMU);
    }
    if cfg!(target_os = "macos") {
        notices.push(MOLTENVK);
    }
    notices.push(RCHEEVOS);
    notices.push(SLINT);
    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emulator_romp_downloads_has_its_license() {
        for core in all_cores() {
            assert!(
                core_notice(core).is_some(),
                "{} has no license entry",
                core.id
            );
        }
        let names: Vec<_> = notices().iter().map(|n| n.name).collect();
        assert_eq!(names[0], "Romp");
        assert!(names.contains(&"Slint"));
        assert!(notices().iter().all(|n| n.url.starts_with("https://")));
    }
}
