//! Map-list requests use only an owned disposable PG17 schema and synthetic rows.
use crate::{
    AppState, Router,
    maps::map_mutation_tests::{MutationFixture, actor},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::Value;
use tower::ServiceExt;

async fn seed_map(
    state: &AppState,
    index: i64,
    version: &str,
    mode: i16,
    source: &str,
    status: &str,
) -> i64 {
    let official_id = (source == "official").then_some(70_000 + index);
    sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path,archive_path,created_at) VALUES($1,$2,'Fixture Song','Fixture Artist','Fixture Creator',$3,$4,$5,$6,1,$7,$8,$9,'2026-01-01T00:00:00Z') RETURNING id")
        .bind(official_id)
        .bind(official_id.map(|_| 80_000))
        .bind(version)
        .bind(mode)
        .bind(status)
        .bind(source)
        .bind(format!("{index:032x}"))
        .bind(format!("private-fixture-{index}.osu"))
        .bind(format!("private-fixture-{index}.osz"))
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
    let items = value["items"].as_array().unwrap();
    let ids = items
        .iter()
        .map(|item| item["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
    for item in items {
        assert!(item.get("osu_path").is_none());
        assert!(item.get("archive_path").is_none());
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn difficulty_search_preserves_filters_pagination_and_admin_authorization() {
    let fixture = MutationFixture::new("map_search", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, staff) = actor(&state, "MapSearchStaff", &["admin.view"]).await;
        let (_, _, _, player) = actor(&state, "MapSearchPlayer", &["beatmaps.upload"]).await;
        let first = seed_map(&state, 1, "Maximum 星降り I", 0, "local", "ranked").await;
        let second = seed_map(&state, 2, "Maximum 星降り II", 0, "local", "ranked").await;
        let third = seed_map(&state, 3, "Maximum 星降り III", 0, "local", "ranked").await;
        let official = seed_map(&state, 4, "Maximum 星降り IV", 0, "official", "ranked").await;
        let pending = seed_map(&state, 5, "Maximum 星降り V", 0, "local", "pending").await;
        let mania = seed_map(&state, 6, "Maximum 星降り VI", 3, "local", "ranked").await;
        let normal = seed_map(&state, 7, "普通", 0, "local", "ranked").await;
        let quoted = seed_map(&state, 8, "O'Brien 展望", 0, "local", "ranked").await;

        let mut public_pages = Vec::new();
        for (endpoint, cookie) in [
            ("/api/v1/beatmaps", None),
            ("/api/v1/admin/beatmaps", Some(staff.as_str())),
        ] {
            // Only versions contain this mixed-case, Unicode query. Each
            // distractor differs in exactly one of the three active filters.
            for (page, expected) in [
                ("1", vec![third, second]),
                ("2", vec![first]),
                ("3", vec![]),
            ] {
                let (status, value) = get(
                    &app,
                    &path(
                        endpoint,
                        &[
                            ("q", "mAxImUm 星降り"),
                            ("source", "local"),
                            ("status", "ranked"),
                            ("mode", "0"),
                            ("page_size", "2"),
                            ("page", page),
                        ],
                    ),
                    cookie,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &expected, 3, page.parse().unwrap(), 2);
                if cookie.is_none() {
                    public_pages.push(value);
                } else {
                    assert_eq!(value, public_pages[page.parse::<usize>().unwrap() - 1]);
                }
            }
            for (source, status_filter, mode, expected) in [
                ("official", "ranked", "0", official),
                ("local", "pending", "0", pending),
                ("local", "ranked", "3", mania),
            ] {
                let (status, value) = get(
                    &app,
                    &path(
                        endpoint,
                        &[
                            ("q", "MAXIMUM 星降り"),
                            ("source", source),
                            ("status", status_filter),
                            ("mode", mode),
                        ],
                    ),
                    cookie,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &[expected], 1, 1, 24);
            }
            let (status, value) = get(&app, &path(endpoint, &[("q", "星降り")]), cookie).await;
            assert_eq!(status, StatusCode::OK);
            assert_page(
                &value,
                &[mania, pending, official, third, second, first],
                6,
                1,
                24,
            );

            // Existing title/artist/creator matches and an omitted query retain
            // the same result set and stable ordering under combined filters.
            for query in [
                None,
                Some("fixture SONG"),
                Some("fixture ARTIST"),
                Some("fixture CREATOR"),
            ] {
                let mut parameters = vec![("source", "local"), ("status", "ranked"), ("mode", "0")];
                if let Some(query) = query {
                    parameters.push(("q", query));
                }
                let (status, value) = get(&app, &path(endpoint, &parameters), cookie).await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &[quoted, normal, third, second, first], 5, 1, 24);
            }
            for (query, expected) in [("O'Brien 展望", vec![quoted]), ("' OR true --", vec![])] {
                let (status, value) = get(&app, &path(endpoint, &[("q", query)]), cookie).await;
                assert_eq!(status, StatusCode::OK);
                assert_page(&value, &expected, expected.len() as i64, 1, 24);
            }
            let (status, value) =
                get(&app, &path(endpoint, &[("q", &"a".repeat(101))]), cookie).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(value["error"]["code"], "invalid_request");
        }

        let admin_path = path("/api/v1/admin/beatmaps", &[("q", "Maximum 星降り")]);
        for (cookie, expected_status, expected_code) in [
            (None, StatusCode::UNAUTHORIZED, "unauthorized"),
            (Some(player.as_str()), StatusCode::FORBIDDEN, "forbidden"),
        ] {
            let (status, value) = get(&app, &admin_path, cookie).await;
            assert_eq!(status, expected_status);
            assert_eq!(value["error"]["code"], expected_code);
            assert!(value.get("items").is_none());
        }
        let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(audit_count, 0, "search requests must remain read-only");
    })
    .await;
    fixture.finish(result).await;
}
