//! Uploader identity filters use an owned disposable PG17 schema only.
use super::{map_mutation_tests::MutationFixture, uploader_filter};
use crate::{AppState, Router, maps::map_mutation_tests::actor};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::Value;
use tower::ServiceExt;

#[test]
fn uploader_ids_are_optional_positive_i64_values() {
    assert_eq!(uploader_filter(None).unwrap(), None);
    for (raw, expected) in [("1", 1), ("24", 24), ("9223372036854775807", i64::MAX)] {
        assert_eq!(uploader_filter(Some(raw)).unwrap(), Some(expected));
    }
    for raw in [
        "",
        "0",
        "-1",
        "-9223372036854775808",
        "9223372036854775808",
        "user",
        "1.5",
        " 1",
        "1 OR true",
    ] {
        let error = uploader_filter(Some(raw)).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST, "{raw}");
        assert_eq!(error.1, "invalid_request", "{raw}");
    }
}

async fn seed_map(
    state: &AppState,
    index: i64,
    uploader: Option<i64>,
    source: &str,
    status: &str,
    mode: i16,
) -> i64 {
    let official_id = (source == "official").then_some(70_000 + index);
    sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,md5,osu_path,created_at) VALUES($1,$2,$3,'Fixture Song','Fixture Artist','UploadOwner','星降り',$4,$5,$6,1,$7,$8,'2026-01-01T00:00:00Z') RETURNING id")
        .bind(official_id)
        .bind(official_id.map(|_| 80_000))
        .bind(uploader)
        .bind(mode)
        .bind(status)
        .bind(source)
        .bind(format!("{index:032x}"))
        .bind(format!("uploader-fixture-{index}.osu"))
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

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
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
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn assert_page(value: &Value, expected: &[i64], total: i64, page: i64, size: i64) {
    assert_eq!(value["total"], total);
    assert_eq!(value["page"], page);
    assert_eq!(value["page_size"], size);
    let ids = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert!(item.get("osu_path").is_none());
            assert!(item.get("archive_path").is_none());
            item["id"].as_i64().unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn exact_uploader_filter_preserves_catalogue_visibility_and_pagination() {
    let fixture = MutationFixture::new("uploaded_map_list", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, _) = actor(&state, "UploadOwner", &[]).await;
        let (other, _, _, _) = actor(&state, "OtherUploadOwner", &[]).await;
        let (empty_owner, _, _, _) = actor(&state, "EmptyUploadOwner", &[]).await;
        let (staff, _, _, staff_cookie) = actor(&state, "UploadListStaff", &["admin.view"]).await;
        let first = seed_map(&state, 1, Some(owner), "local", "ranked", 0).await;
        let second = seed_map(&state, 2, Some(owner), "local", "ranked", 0).await;
        let third = seed_map(&state, 3, Some(owner), "local", "ranked", 0).await;
        let official = seed_map(&state, 4, Some(owner), "official", "ranked", 0).await;
        let pending = seed_map(&state, 5, Some(owner), "local", "pending", 0).await;
        let mania = seed_map(&state, 6, Some(owner), "local", "ranked", 3).await;
        let withdrawn = seed_map(&state, 7, Some(owner), "local", "ranked", 0).await;
        let others = seed_map(&state, 8, Some(other), "local", "ranked", 0).await;
        let unowned = seed_map(&state, 9, None, "local", "ranked", 0).await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) SELECT native_set_id,true,1,'Owned fixture withdrawal',$2 FROM beatmaps WHERE id=$1")
            .bind(withdrawn)
            .bind(staff)
            .execute(&state.db)
            .await
            .unwrap();
        let owner_id = owner.to_string();

        for (endpoint, cookie) in [
            ("/api/v1/beatmaps", None),
            ("/api/v1/admin/beatmaps", Some(staff_cookie.as_str())),
        ] {
            for (page, expected) in [("1", vec![third, second]), ("2", vec![first]), ("3", vec![])] {
                let (status, value) = get(&app, &path(endpoint, &[
                    ("uploaded_by", &owner_id), ("source", "local"), ("status", "ranked"),
                    ("mode", "0"), ("q", "星降り"), ("page_size", "2"), ("page", page),
                ]), cookie).await;
                assert_eq!(status, StatusCode::OK, "{value}");
                assert_page(&value, &expected, 3, page.parse().unwrap(), 2);
                for item in value["items"].as_array().unwrap() {
                    assert_eq!(item["uploaded_by"], owner);
                    assert_eq!(item["availability"]["withdrawn"], false);
                }
            }
            for (source, status_filter, mode, expected) in [
                ("official", "ranked", "0", official),
                ("local", "pending", "0", pending),
                ("local", "ranked", "3", mania),
            ] {
                let (status, value) = get(&app, &path(endpoint, &[
                    ("uploaded_by", &owner_id), ("source", source),
                    ("status", status_filter), ("mode", mode),
                ]), cookie).await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &[expected], 1, 1, 24);
            }
            let (status, value) = get(&app, &path(endpoint, &[("uploaded_by", &owner_id)]), cookie).await;
            assert_eq!(status, StatusCode::OK);
            assert_page(&value, &[mania, pending, official, third, second, first], 6, 1, 24);

            // Identical creator metadata on every row never grants upload ownership.
            for (id, expected) in [(other, vec![others]), (empty_owner, vec![]), (i64::MAX, vec![])] {
                let (status, value) = get(&app, &path(endpoint, &[
                    ("uploaded_by", &id.to_string()), ("source", "local"), ("q", "UploadOwner"),
                ]), cookie).await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &expected, expected.len() as i64, 1, 24);
            }
            // Omitting ownership retains existing catalogue semantics, including unowned content.
            let (status, value) = get(&app, &path(endpoint, &[
                ("source", "local"), ("status", "ranked"), ("mode", "0"), ("q", "UploadOwner"),
            ]), cookie).await;
            assert_eq!(status, StatusCode::OK);
            assert_page(&value, &[unowned, others, third, second, first], 5, 1, 24);

            for raw in ["", "0", "-1", "not-an-id", "9223372036854775808", "1 OR true"] {
                let (status, value) = get(&app, &path(endpoint, &[("uploaded_by", raw)]), cookie).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{raw}: {value}");
                assert_eq!(value["error"]["code"], "invalid_request");
                assert!(value.get("items").is_none());
            }
        }
        // Public query parameters cannot expose withdrawn rows; staff retain their existing filter.
        let (status, value) = get(&app, &path("/api/v1/beatmaps", &[
            ("uploaded_by", &owner_id), ("availability", "all"),
        ]), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_page(&value, &[mania, pending, official, third, second, first], 6, 1, 24);
        let (status, value) = get(&app, &path("/api/v1/admin/beatmaps", &[
            ("uploaded_by", &owner_id), ("availability", "withdrawn"),
        ]), Some(&staff_cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert_page(&value, &[withdrawn], 1, 1, 24);
        assert_eq!(value["items"][0]["availability"]["withdrawn"], true);
        let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
            .fetch_one(&state.db).await.unwrap();
        assert_eq!(audit_count, 0, "uploader listings remain read-only");
    }).await;
    fixture.finish(result).await;
}
