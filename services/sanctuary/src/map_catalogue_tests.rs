//! Catalogue fixtures own a disposable PG17 schema and never contact upstream.
use crate::{
    AppState, Router,
    maps::map_mutation_tests::{MULTIPART, MutationFixture, actor, multipart, osu_bytes, request},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::io::Cursor;
use tower::ServiceExt;

struct Seed<'a> {
    index: i64,
    set: Option<i64>,
    title: &'a str,
    version: &'a str,
    mode: i16,
    source: &'a str,
    status: &'a str,
    stars: f64,
    length: i32,
    uploader: Option<i64>,
    created: &'a str,
}
impl Seed<'static> {
    fn official(index: i64, set: i64) -> Self {
        Self {
            index,
            set: Some(set),
            title: "Same Fixture Song",
            version: "Easy",
            mode: 0,
            source: "official",
            status: "ranked",
            stars: 1.0,
            length: 30,
            uploader: None,
            created: "2026-01-01T00:00:00Z",
        }
    }
    fn local(index: i64) -> Self {
        Self {
            set: None,
            source: "local",
            ..Self::official(index, 1)
        }
    }
}
async fn seed(state: &AppState, map: Seed<'_>) -> i64 {
    sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,length_seconds,bpm,max_combo,md5,osu_path,created_at) VALUES($1,$2,$3,$4,'Fixture Artist','Fixture Creator',$5,$6,$7,$8,$9,$10,120,42,$11,$12,$13::timestamptz) RETURNING id")
        .bind(map.set.map(|_| 100_000 + map.index)).bind(map.set).bind(map.uploader)
        .bind(map.title).bind(map.version).bind(map.mode).bind(map.status).bind(map.source)
        .bind(map.stars).bind(map.length).bind(format!("{:032x}",map.index))
        .bind(format!("private-fixture-{}.osu",map.index))
        .bind(chrono::DateTime::parse_from_rfc3339(map.created).unwrap().with_timezone(&chrono::Utc))
        .fetch_one(&state.db).await.unwrap()
}
async fn native_set(state: &AppState, id: i64) -> i64 {
    sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap()
}
fn path(endpoint: &str, parameters: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("http://fixture.invalid").unwrap();
    url.query_pairs_mut()
        .extend_pairs(parameters.iter().copied());
    format!("{endpoint}?{}", url.query().unwrap())
}
async fn get_bytes(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}
async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
    let (status, bytes) = get_bytes(app, path, cookie).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn catalogue(app: &Router, parameters: &[(&str, &str)]) -> Value {
    let (status, value) = get(app, &path("/api/v1/beatmapsets", parameters), None).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
fn ids(page: &Value, field: &str) -> Vec<i64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item[field].as_i64().unwrap())
        .collect()
}
fn assert_page(value: &Value, expected: &[i64], field: &str, total: i64, page: i64, size: i64) {
    assert_eq!(ids(value, field), expected);
    assert_eq!(value["total"], total);
    assert_eq!(value["page"], page);
    assert_eq!(value["page_size"], size);
}
fn assert_keys(value: &Value, expected: &[&str]) {
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    keys.sort_unstable();
    expected.sort_unstable();
    assert_eq!(keys, expected);
}
fn assert_difficulty(value: &Value) {
    assert_keys(
        value,
        &[
            "id",
            "title",
            "artist",
            "creator",
            "version",
            "mode",
            "status",
            "source",
            "stars",
            "length_seconds",
            "bpm",
            "max_combo",
        ],
    );
}
fn assert_public_map(value: &Value) {
    assert!(value.get("osu_path").is_none());
    assert!(value.get("archive_path").is_none());
    assert!(value["availability"].get("reason").is_none());
    assert!(value["availability"].get("actor_id").is_none());
    for private in ["sha256", "member_md5", "archive_path", "osu_path"] {
        assert!(value["package"].get(private).is_none());
    }
}
async fn snapshot(state: &AppState) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('maps',(SELECT COALESCE(jsonb_agg(to_jsonb(b) ORDER BY id),'[]'::jsonb) FROM beatmaps b),'packages',(SELECT COALESCE(jsonb_agg(to_jsonb(p) ORDER BY id),'[]'::jsonb) FROM beatmap_packages p),'availability',(SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY native_set_id),'[]'::jsonb) FROM beatmapset_availability a),'archives',(SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY official_set_id),'[]'::jsonb) FROM official_beatmapset_archives a),'audits',(SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY id),'[]'::jsonb) FROM audit_events a),'updates',(SELECT COALESCE(jsonb_agg(to_jsonb(u) ORDER BY id),'[]'::jsonb) FROM lazer_beatmap_updates u))")
        .fetch_one(&state.db).await.unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_groups_only_canonical_sets_and_counts_empty_pages() {
    let fixture = MutationFixture::new("catalogue_identity", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let first = seed(&state, Seed::official(1, 800)).await;
        let mut sibling = Seed::official(2, 800);
        sibling.source = "local"; // Legacy source labels do not change set identity.
        seed(&state, sibling).await;
        seed(&state, Seed::official(3, 900)).await;
        let local_a = seed(&state, Seed::local(4)).await;
        let local_b = seed(&state, Seed::local(5)).await;
        let local_a = native_set(&state, local_a).await;
        let local_b = native_set(&state, local_b).await;
        assert_ne!(
            local_a, local_b,
            "identical titles must not merge standalone uploads"
        );
        let before = snapshot(&state).await;
        let all = catalogue(&app, &[]).await;
        assert_page(
            &all,
            &[800, 900, local_a, local_b],
            "native_set_id",
            4,
            1,
            24,
        );
        assert_eq!(all["items"][0]["representative"]["id"], first);
        assert_eq!(all["items"][0]["matching_count"], 2);
        assert_eq!(all["items"][0]["stored_count"], 2);
        for (page, expected) in [
            ("1", vec![800, 900]),
            ("2", vec![local_a, local_b]),
            ("3", vec![]),
        ] {
            let value = catalogue(&app, &[("page", page), ("page_size", "2")]).await;
            assert_page(
                &value,
                &expected,
                "native_set_id",
                4,
                page.parse().unwrap(),
                2,
            );
        }
        assert_page(
            &catalogue(&app, &[("q", "no saved difficulty matches")]).await,
            &[],
            "native_set_id",
            0,
            1,
            24,
        );
        assert_page(
            &catalogue(&app, &[("page", "4294967295"), ("page_size", "4294967295")]).await,
            &[],
            "native_set_id",
            4,
            1_000_000,
            100,
        );
        assert_page(
            &catalogue(&app, &[("page", "0"), ("page_size", "0")]).await,
            &[800],
            "native_set_id",
            4,
            1,
            1,
        );
        assert_eq!(snapshot(&state).await, before);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_filters_matching_rows_before_grouping_and_summary() {
    let fixture = MutationFixture::new("catalogue_filters", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, _) = actor(&state, "CatalogueOwner", &[]).await;
        let (other, _, _, _) = actor(&state, "CatalogueOther", &[]).await;
        let mut unmatched = Seed::official(1, 800);
        unmatched.version = "Ordinary";
        unmatched.stars = 9.0;
        unmatched.length = 900;
        unmatched.created = "2026-09-01T00:00:00Z";
        seed(&state, unmatched).await;
        let mut expected = Vec::new();
        for (index, mode, source, status, uploader) in [
            (2, 0, "local", "ranked", owner),
            (3, 0, "local", "ranked", owner),
            (4, 3, "official", "loved", other),
            (5, 0, "local", "pending", other),
            (6, 1, "official", "unranked", owner),
        ] {
            let mut map = Seed::official(index, 800);
            map.version = "Maximum 星降り O'Brien";
            map.mode = mode;
            map.source = source;
            map.status = status;
            map.uploader = Some(uploader);
            map.stars = index as f64;
            map.length = index as i32 * 10;
            let id = seed(&state, map).await;
            if index <= 3 {
                expected.push(id);
            }
        }
        let uploader = owner.to_string();
        let value = catalogue(
            &app,
            &[
                ("q", "mAxImUm 星降り O'Brien"),
                ("mode", "0"),
                ("source", "local"),
                ("status", "ranked"),
                ("uploaded_by", &uploader),
            ],
        )
        .await;
        let set = &value["items"][0];
        assert_eq!(value["total"], 1);
        assert_eq!(set["matching_count"], 2);
        assert_eq!(set["stored_count"], 6);
        assert_eq!(set["representative"]["id"], expected[0]);
        assert_eq!(set["matching_modes"], json!([0]));
        assert_eq!(set["matching_statuses"], json!(["ranked"]));
        assert_eq!(set["stars_min"], 2.0);
        assert_eq!(set["stars_max"], 3.0);
        assert_eq!(set["length_min"], 20);
        assert_eq!(set["length_max"], 30);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(set["added_at"].as_str().unwrap())
                .unwrap()
                .timestamp(),
            chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .timestamp()
        );
        let preview = json!({"items": set["preview"]});
        assert_eq!(ids(&preview, "id"), expected);
        for (parameters, count, modes, statuses) in [
            (
                vec![("q", "星降り")],
                5,
                json!([0, 1, 3]),
                json!(["loved", "pending", "ranked", "unranked"]),
            ),
            (
                vec![("q", "星降り"), ("uploaded_by", uploader.as_str())],
                3,
                json!([0, 1]),
                json!(["ranked", "unranked"]),
            ),
            (
                vec![("q", "星降り"), ("source", "official")],
                2,
                json!([1, 3]),
                json!(["loved", "unranked"]),
            ),
            (
                vec![("q", "星降り"), ("status", "pending")],
                1,
                json!([0]),
                json!(["pending"]),
            ),
        ] {
            let page = catalogue(&app, &parameters).await;
            assert_eq!(page["items"][0]["matching_count"], count);
            assert_eq!(page["items"][0]["stored_count"], 6);
            assert_eq!(page["items"][0]["matching_modes"], modes);
            assert_eq!(page["items"][0]["matching_statuses"], statuses);
        }
        for q in ["fixture ARTIST", "fixture CREATOR", "same fixture SONG"] {
            assert_eq!(
                catalogue(&app, &[("q", q)]).await["items"][0]["matching_count"],
                6
            );
        }
        assert_eq!(catalogue(&app, &[("q", "' OR true --")]).await["total"], 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_preview_is_bounded_ordered_and_uses_exact_public_shapes() {
    let fixture = MutationFixture::new("catalogue_preview", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let mut order = Vec::new();
        for index in 1..=12 {
            let mut map = Seed::official(index, 800);
            map.mode = ((index - 1) % 4) as i16;
            map.stars = (12 - index) as f64;
            let id = seed(&state, map).await;
            order.push((map_mode(index), (12 - index), id));
        }
        order.sort_unstable();
        let value = catalogue(&app, &[]).await;
        let set = &value["items"][0];
        assert_keys(
            set,
            &[
                "native_set_id",
                "representative",
                "matching_count",
                "stored_count",
                "matching_modes",
                "matching_statuses",
                "stars_min",
                "stars_max",
                "length_min",
                "length_max",
                "added_at",
                "preview",
            ],
        );
        assert_eq!(set["matching_count"], 12);
        assert_eq!(set["stored_count"], 12);
        assert_eq!(set["representative"]["id"], 1);
        assert_eq!(set["preview"].as_array().unwrap().len(), 8);
        assert_eq!(
            ids(&json!({"items":set["preview"]}), "id"),
            order
                .iter()
                .take(8)
                .map(|entry| entry.2)
                .collect::<Vec<_>>()
        );
        assert_public_map(&set["representative"]);
        assert_eq!(
            get(&app, "/api/v1/beatmaps/1", None).await.1,
            set["representative"],
            "catalogue representatives preserve the established public map DTO"
        );
        for difficulty in set["preview"].as_array().unwrap() {
            assert_difficulty(difficulty);
        }
        for unknown in [
            "source",
            "status",
            "tags",
            "ar",
            "od",
            "cs",
            "hp",
            "submitted_date",
            "ranked_date",
        ] {
            assert!(set.get(unknown).is_none());
        }
        assert_eq!(set["representative"]["package"], Value::Null);
        assert_eq!(set["representative"]["download_kind"], "osu");
    })
    .await;
    fixture.finish(result).await;
}
fn map_mode(index: i64) -> i16 {
    ((index - 1) % 4) as i16
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_sorts_matching_aggregates_with_canonical_ties() {
    let fixture = MutationFixture::new("catalogue_sort", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        for (index, set, title, stars, length, created) in [
            (1, 300, "Zebra", 1.0, 50, "2026-01-01T00:00:00Z"),
            (2, 200, "Alpha", 2.0, 100, "2026-02-01T00:00:00Z"),
            (3, 100, "Alpha", 2.0, 100, "2026-02-01T00:00:00Z"),
            (4, 300, "A sibling title", 8.0, 500, "2026-03-01T00:00:00Z"),
        ] {
            let mut map = Seed::official(index, set);
            map.title = title;
            map.stars = stars;
            map.length = length;
            map.created = created;
            map.version = if index == 4 { "Excluded" } else { "Included" };
            seed(&state, map).await;
        }
        for (sort, expected) in [
            ("recent", vec![300, 100, 200]),
            ("title", vec![100, 200, 300]),
            ("stars", vec![300, 100, 200]),
            ("length", vec![300, 100, 200]),
        ] {
            let page = catalogue(&app, &[("sort", sort)]).await;
            assert_page(&page, &expected, "native_set_id", 3, 1, 24);
            assert_eq!(catalogue(&app, &[("sort", sort)]).await, page);
            assert_page(
                &catalogue(&app, &[("sort", sort), ("page", "2"), ("page_size", "1")]).await,
                &[expected[1]],
                "native_set_id",
                3,
                2,
                1,
            );
        }
        for sort in ["recent", "stars", "length", "title"] {
            assert_page(
                &catalogue(&app, &[("sort", sort), ("q", "Included")]).await,
                &[100, 200, 300],
                "native_set_id",
                3,
                1,
                24,
            );
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_rejects_invalid_filters_and_bounds_utf8_bytes() {
    let fixture = MutationFixture::new("catalogue_validation", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        seed(&state, Seed::official(1, 800)).await;
        let before = snapshot(&state).await;
        for (field, value) in [
            ("mode", "-1"),
            ("mode", "4"),
            ("source", "upstream"),
            ("source", "LOCAL"),
            ("status", "qualified"),
            ("status", "RANKED"),
            ("sort", "popular"),
            ("sort", ""),
            ("uploaded_by", "0"),
            ("uploaded_by", "-1"),
            ("uploaded_by", "word"),
            ("uploaded_by", "9223372036854775808"),
            ("uploaded_by", ""),
        ] {
            let (status, value) =
                get(&app, &path("/api/v1/beatmapsets", &[(field, value)]), None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}={value}");
            assert_eq!(value["error"]["code"], "invalid_request");
            assert!(value.get("items").is_none());
        }
        for query in ["a".repeat(101), "星".repeat(34)] {
            let (status, value) =
                get(&app, &path("/api/v1/beatmapsets", &[("q", &query)]), None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(value["error"]["code"], "invalid_request");
        }
        for query in ["a".repeat(100), format!("{}a", "星".repeat(33))] {
            assert_eq!(catalogue(&app, &[("q", &query)]).await["total"], 0);
        }
        for (field, value) in [
            ("mode", "nonnumeric"),
            ("mode", "32768"),
            ("page", "-1"),
            ("page_size", "4294967296"),
        ] {
            assert_eq!(
                get(&app, &path("/api/v1/beatmapsets", &[(field, value)]), None)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(snapshot(&state).await, before);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_set_selector_works_for_cached_siblings_without_archive() {
    let fixture = MutationFixture::new("catalogue_selector", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let mut expected = Vec::new();
        for (index, mode, stars) in [(1, 3, 1.0), (2, 0, 3.0), (3, 0, 1.0), (4, 0, 1.0)] {
            let mut map = Seed::official(index, 800);
            map.mode = mode;
            map.stars = stars;
            expected.push(seed(&state, map).await);
        }
        let anchors = expected.clone();
        expected = vec![expected[2], expected[3], expected[1], expected[0]];
        let standalone = seed(&state, Seed::local(5)).await;
        let before = snapshot(&state).await;
        let mut first = None;
        for anchor in &anchors {
            let endpoint = format!("/api/v1/beatmaps/{anchor}/set/difficulties");
            let (status, value) = get(&app, &endpoint, None).await;
            assert_eq!(status, StatusCode::OK);
            assert_page(&value, &expected, "id", 4, 1, 24);
            for item in value["items"].as_array().unwrap() {
                assert_difficulty(item);
            }
            if let Some(first) = &first {
                assert_eq!(&value, first);
            } else {
                first = Some(value);
            }
            assert_page(
                &get(
                    &app,
                    &path(&endpoint, &[("page", "2"), ("page_size", "2")]),
                    None,
                )
                .await
                .1,
                &expected[2..],
                "id",
                4,
                2,
                2,
            );
            assert_page(
                &get(
                    &app,
                    &path(&endpoint, &[("page", "3"), ("page_size", "2")]),
                    None,
                )
                .await
                .1,
                &[],
                "id",
                4,
                3,
                2,
            );
        }
        assert_page(
            &get(
                &app,
                &format!("/api/v1/beatmaps/{standalone}/set/difficulties"),
                None,
            )
            .await
            .1,
            &[standalone],
            "id",
            1,
            1,
            24,
        );
        let sets = catalogue(&app, &[]).await;
        let cached = &sets["items"][0]["representative"];
        assert_eq!(cached["package"], Value::Null);
        assert_eq!(cached["download_kind"], "osu");
        assert_public_map(cached);
        assert_eq!(
            get(&app, "/api/v1/beatmaps/999999/set/difficulties", None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(snapshot(&state).await, before);
        sqlx::query("DELETE FROM beatmaps WHERE id=$1")
            .bind(expected[1])
            .execute(&state.db)
            .await
            .unwrap();
        expected.remove(1);
        assert_page(
            &get(
                &app,
                &format!("/api/v1/beatmaps/{}/set/difficulties", anchors[0]),
                None,
            )
            .await
            .1,
            &expected,
            "id",
            3,
            1,
            24,
        );
        let set = &catalogue(&app, &[]).await["items"][0];
        assert_eq!(set["stored_count"], 3);
        assert_eq!(set["matching_count"], 3);
    })
    .await;
    fixture.finish(result).await;
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
async fn upload(app: &Router, cookie: &str, name: &str, bytes: &[u8]) -> Value {
    let (status, value) = request(
        app,
        "/api/v1/beatmaps/upload",
        Some(cookie),
        false,
        MULTIPART,
        multipart(name, bytes),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_preserves_standalone_and_archive_download_truth() {
    let fixture = MutationFixture::new("catalogue_packages", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "CatalogueUploader", &["beatmaps.upload"]).await;
        let single_bytes = osu_bytes("Catalogue Standalone");
        let single = upload(&app, &cookie, "standalone.osu", &single_bytes).await;
        let easy = osu_bytes("Catalogue Archive");
        let hard = String::from_utf8(easy.clone())
            .unwrap()
            .replace("Version:Easy", "Version:Hard")
            .into_bytes();
        let bytes = archive(&[("Easy.osu", &easy), ("Hard.osu", &hard)]);
        let uploaded = upload(&app, &cookie, "package.osz", &bytes).await;
        let single_id = single["items"][0]["id"].as_i64().unwrap();
        let package_ids = uploaded["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_i64().unwrap())
            .collect::<Vec<_>>();
        let archive_set = uploaded["package"]["native_set_id"].as_i64().unwrap();
        let before = snapshot(&state).await;
        let catalogue_page = catalogue(&app, &[]).await;
        for (id, kind, count, content) in [
            (single_id, "osu", 1, &single_bytes),
            (package_ids[0], "osz", 2, &bytes),
        ] {
            let set = catalogue_page["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|set| set["representative"]["id"] == id)
                .unwrap();
            let representative = &set["representative"];
            assert_public_map(representative);
            assert_eq!(
                get(&app, &format!("/api/v1/beatmaps/{id}"), None).await.1,
                *representative
            );
            assert_eq!(representative["download_kind"], kind);
            assert_eq!(representative["package"]["kind"], kind);
            assert_eq!(representative["package"]["difficulty_count"], count);
            assert_eq!(set["stored_count"], count);
            let (status, download) =
                get_bytes(&app, representative["download_url"].as_str().unwrap(), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(&download, content);
        }
        assert_eq!(
            snapshot(&state).await,
            before,
            "catalogue and downloads are read-only"
        );
        sqlx::query("DELETE FROM beatmaps WHERE id=$1")
            .bind(package_ids[1])
            .execute(&state.db)
            .await
            .unwrap();
        let value = catalogue(&app, &[]).await;
        let set = value["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|set| set["native_set_id"] == archive_set)
            .unwrap();
        assert_eq!(set["stored_count"], 1);
        assert_eq!(set["matching_count"], 1);
        assert_eq!(
            set["representative"]["package"]["difficulty_count"], 2,
            "retained archive manifest still describes its complete bytes"
        );
        assert_eq!(set["representative"]["download_kind"], "osz");
        assert_eq!(
            get_bytes(
                &app,
                set["representative"]["download_url"].as_str().unwrap(),
                None
            )
            .await
            .1,
            bytes
        );
        assert_page(
            &get(
                &app,
                &format!("/api/v1/beatmaps/{}/set/difficulties", package_ids[0]),
                None,
            )
            .await
            .1,
            &[package_ids[0]],
            "id",
            1,
            1,
            24,
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn catalogue_withdrawal_is_public_even_with_staff_cookie_and_all_filter() {
    let fixture = MutationFixture::new("catalogue_withdrawal", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let (staff_id,_,_,cookie)=actor(&state,"CatalogueStaff",&["admin.view","beatmaps.moderate"]).await;
        let hidden=seed(&state,Seed::official(1,800)).await;
        let sibling=seed(&state,Seed::official(2,800)).await;
        let active=seed(&state,Seed::official(3,900)).await;
        let local=seed(&state,Seed::local(4)).await;
        let local_set=native_set(&state,local).await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(800,true,1,'Private catalogue moderation reason',$1),(900,true,1,'Private restored moderation reason',$1),($2,true,1,'Private local moderation reason',$1)").bind(staff_id).bind(local_set).execute(&state.db).await.unwrap();
        sqlx::query("UPDATE beatmapset_availability SET withdrawn=false,revision=2 WHERE native_set_id=900").execute(&state.db).await.unwrap();
        let before=snapshot(&state).await;
        let official_cache_before=state.official.lock().await.metadata_cache_shape();
        for cookie in [None,Some(cookie.as_str())] {
            for availability in ["active","withdrawn","all"] {
                let (status,value)=get(&app,&path("/api/v1/beatmapsets",&[("availability",availability)]),cookie).await;
                assert_eq!(status,StatusCode::OK);
                assert_page(&value,&[900],"native_set_id",1,1,24);
                let representative=&value["items"][0]["representative"];
                assert_eq!(representative["id"],active);
                assert_eq!(representative["availability"]["revision"],2);
                assert_eq!(representative["availability"]["withdrawn"],false);
                assert_public_map(representative);
                let encoded=value.to_string();
                for secret in ["Private catalogue moderation reason","Private restored moderation reason","private-fixture","actor_id"] {assert!(!encoded.contains(secret),"public leak: {secret}");}
            }
            for anchor in [hidden,sibling,local] {
                let (status,value)=get(&app,&format!("/api/v1/beatmaps/{anchor}/set/difficulties?availability=all"),cookie).await;
                assert_eq!(status,StatusCode::GONE);
                assert_eq!(value["error"]["code"],"beatmap_withdrawn");
                assert!(value.get("items").is_none());
                assert!(!value.to_string().contains("Private"));
            }
        }
        assert_eq!(snapshot(&state).await,before);
        assert_eq!(state.official.lock().await.metadata_cache_shape(),official_cache_before);
    }).await;
    fixture.finish(result).await;
}
