use yogurt_db::{settings, Db};

#[test]
fn it_returns_seeded_defaults() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(
        settings::get(&db, "general.port").unwrap().as_deref(),
        Some("7878")
    );
    assert_eq!(
        settings::get(&db, "general.open_browser_on_start")
            .unwrap()
            .as_deref(),
        Some("true")
    );
}

#[test]
fn it_upserts_a_value() {
    let db = Db::open_in_memory().unwrap();
    settings::set(&db, "general.port", "9000").unwrap();
    assert_eq!(
        settings::get(&db, "general.port").unwrap().as_deref(),
        Some("9000")
    );
    settings::set(&db, "general.port", "9001").unwrap();
    assert_eq!(
        settings::get(&db, "general.port").unwrap().as_deref(),
        Some("9001")
    );
}

#[test]
fn it_loads_typed_general_struct() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::load_general(&db).unwrap();
    assert_eq!(g.port, 7878);
    assert!(g.open_browser_on_start);
    assert_eq!(g.audio_input_device, "");
    assert!(!g.first_run_completed);
}

#[test]
fn it_flips_first_run_completed_via_patch() {
    let db = Db::open_in_memory().unwrap();
    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            first_run_completed: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(patched.first_run_completed);
    assert!(settings::load_general(&db).unwrap().first_run_completed);
}

#[test]
fn it_saves_a_general_patch_and_returns_updated() {
    let db = Db::open_in_memory().unwrap();
    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            port: Some(8080),
            open_browser_on_start: Some(false),
            audio_input_device: Some("MacBook Pro Microphone".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(patched.port, 8080);
    assert!(!patched.open_browser_on_start);
    assert_eq!(patched.audio_input_device, "MacBook Pro Microphone");
}

/// V005 seeds defaults, and V027 makes a fresh install's model Parakeet Ultra.
#[test]
fn it_loads_seeded_stt_defaults() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::load_general(&db).unwrap();
    assert_eq!(g.stt_provider, "cloud");
    assert_eq!(g.stt_model, "parakeet-ultra");
}

#[test]
fn it_patches_stt_provider_and_model() {
    let db = Db::open_in_memory().unwrap();
    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            stt_provider: Some("local".into()),
            stt_model: Some("medium.en".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(patched.stt_provider, "local");
    assert_eq!(patched.stt_model, "medium.en");
    let g = settings::load_general(&db).unwrap();
    assert_eq!(g.stt_provider, "local");
    assert_eq!(g.stt_model, "medium.en");
}

#[test]
fn it_loads_meeting_detection_focus_default_and_patches_it() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::load_general(&db).unwrap();
    assert!(g.meeting_detection_focus);

    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            meeting_detection_focus: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!patched.meeting_detection_focus);
    assert!(!settings::load_general(&db).unwrap().meeting_detection_focus);
}

#[test]
fn it_loads_live_recap_default_and_patches_it() {
    let db = Db::open_in_memory().unwrap();
    assert!(settings::load_general(&db).unwrap().live_recap);

    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            live_recap: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!patched.live_recap);
    assert!(!settings::load_general(&db).unwrap().live_recap);
}

#[test]
fn it_loads_recap_placement_default_and_patches_it() {
    use settings::RecapPlacement;
    let db = Db::open_in_memory().unwrap();
    assert_eq!(
        settings::load_general(&db).unwrap().live_recap_placement,
        RecapPlacement::Dock
    );

    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            live_recap_placement: Some(RecapPlacement::Main),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(patched.live_recap_placement, RecapPlacement::Main);

    settings::set(&db, "general.live_recap_placement", "sidebar").unwrap();
    assert_eq!(
        settings::load_general(&db).unwrap().live_recap_placement,
        RecapPlacement::Dock
    );
}

#[test]
fn it_loads_meeting_auto_stop_default_and_patches_it() {
    let db = Db::open_in_memory().unwrap();
    assert!(settings::load_general(&db).unwrap().meeting_auto_stop);

    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            meeting_auto_stop: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!patched.meeting_auto_stop);
    assert!(!settings::load_general(&db).unwrap().meeting_auto_stop);
}

#[test]
fn it_loads_meeting_bubble_default_and_patches_it() {
    let db = Db::open_in_memory().unwrap();
    assert!(settings::load_general(&db).unwrap().meeting_bubble);

    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            meeting_bubble: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!patched.meeting_bubble);
    assert!(!settings::load_general(&db).unwrap().meeting_bubble);
}

#[test]
fn voice_defaults_off_on_fn_and_round_trips() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::load_general(&db).unwrap();
    assert!(!g.voice_enabled);
    assert_eq!(g.voice_hotkey, "fn");
    assert_eq!(g.voice_retention, settings::VoiceRetention::Days30);
    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            voice_enabled: Some(true),
            voice_hotkey: Some("option+e".into()),
            voice_retention: Some(settings::VoiceRetention::Never),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(patched.voice_retention, settings::VoiceRetention::Never);
    assert_eq!(
        serde_json::to_value(settings::VoiceRetention::Hours24).unwrap(),
        "24h"
    );
    assert!(patched.voice_enabled);
    assert_eq!(patched.voice_hotkey, "option+e");
}

#[test]
fn voice_cleanup_defaults_off_on_builtin_and_round_trips() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::load_general(&db).unwrap();
    assert!(!g.voice_cleanup);
    assert_eq!(g.voice_cleanup_provider, settings::VOICE_CLEANUP_BUILTIN);
    let patched = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            voice_cleanup: Some(true),
            voice_cleanup_provider: Some("p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(patched.voice_cleanup);
    assert_eq!(patched.voice_cleanup_provider, "p1");
}

#[test]
fn voice_stt_model_defaults_to_parakeet_and_follows_meetings_only_when_picked() {
    let db = Db::open_in_memory().unwrap();
    let pick = |m: &str| {
        settings::save_general_patch(
            &db,
            settings::GeneralPatch {
                voice_stt_model: Some(m.into()),
                stt_model: Some("large-v3-turbo".into()),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let unset = settings::load_general(&db).unwrap();
    assert_eq!(unset.voice_stt_model, "");
    assert_eq!(unset.voice_model(), "parakeet-ultra");
    assert_eq!(pick("small.en").voice_model(), "small.en");
    assert_eq!(pick("tiny.en").voice_model(), "tiny.en");
    let meetings = pick("meetings");
    assert_eq!(meetings.voice_stt_model, "meetings");
    assert_eq!(meetings.voice_model(), "large-v3-turbo");
    assert_eq!(pick("").voice_model(), "parakeet-ultra");
}

#[test]
fn voice_mic_follows_the_meetings_mic_until_set() {
    let db = Db::open_in_memory().unwrap();
    let g = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            audio_input_device: Some("Studio Mic".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(g.voice_input_device, "");
    assert_eq!(g.voice_mic(), "Studio Mic");
    let g = settings::save_general_patch(
        &db,
        settings::GeneralPatch {
            voice_input_device: Some("Headset Mic".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(g.voice_mic(), "Headset Mic");
}
