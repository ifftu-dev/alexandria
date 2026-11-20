# cmi5 conformance matrix

Status: self-tested against the CATAPULT LMS Test Suite. Claim wording: "cmi5 packaged assignable units, self-tested", never "certified" or "conformant LMS".

Scope: the launch, session and completion rules that apply to assignable units packaged in the archive. Requirements on remote units, on the AU side, and on course export are out of scope (end of this page).

Every row names the automated test that proves it between guillemets. «every_row_of_the_cmi5_matrix_names_a_test_that_exists» fails if a named test is missing. Seams:

- Host seam (Rust, `cargo test -p alexandria-node --lib packages::attempts::cmi5`): `conformance.rs` (this directory) replays the published CATAPULT packages; `tests.rs` drives the player's fixture package. Both import through the app's import path, launch through `launch`, talk real HTTP to the session's loopback endpoint and read back what the store holds.
- Session rules (Rust, `crates/alexandria-studio/tests/cmi5_session.rs`): the pure statement checks the endpoint calls.
- Import (Rust, `crates/alexandria-studio/tests/cmi5_import.rs`, `package_import.rs`).
- Player seam (Playwright, `tests/desktop-ui/cmi5-player.spec.ts`, Chromium and WebKit): the real player and wrapper with a fake endpoint.

Published packages: the CATAPULT LMS Test Suite (`github.com/adlnet/CATAPULT`, `lts/`, Apache-2.0). Runtime packages 001 to 009 are committed as their unmodified course structures in `src-tauri/tests/fixtures/catapult-lts/` and replayed by `lts_*` tests: each test makes the requests that package's unit makes and asserts both the unit's own checks and the suite's post-run checks (`lts/__tests__/runtime.js`). Import packages 101, 102 and 201 to 210 are transcribed in the import tests. Requirement ids are those of the cmi5 requirements list the suite uses (`@cmi5/requirements`); "(d)" ids are the derived LMS tests.

Status values: **passes**; **known deviation** (what the app does instead is pinned by a test titled `known_deviation_…`); **gap** (not implemented, no test claims it); **out of scope**.

## The LTS packages

| Package | Suite checks | Status | Test |
|---|---|---|---|
| 001-essentials | launch URL, fetch, LaunchData, learner preferences, launched context, satisfied order | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 002-allowed | an allowed statement is stored and reported | passes | «lts_002_allowed_statements_are_recorded_and_reported» |
| 003-launchMethod-OwnWindow | the unit runs as the top window | passes on desktop (host side; the window itself at the player seam); known deviation on mobile | «lts_003_an_own_window_unit_gets_its_own_window_on_desktop» «a unit that asks for its own window leaves the frame a placeholder that can bring it forward or end it» |
| 004-1 to 004-4 moveOn | satisfied for block and course right after the statement meeting moveOn | passes | «lts_004_1_completed_satisfies_a_completed_unit» «lts_004_2_completed_satisfies_a_completed_or_passed_unit» «lts_004_3_passed_satisfies_a_passed_unit» «lts_004_4_passed_satisfies_a_completed_or_passed_unit» |
| 004-5 moveOn NotApplicable | satisfied before the unit's first launch | passes | «lts_004_5_a_not_applicable_unit_is_satisfied_when_the_registration_starts» |
| 005-1-invalid-au | 50 requests the launching system must refuse | passes, except two rows that are known deviations (statement id and timestamp filled in; initialized before the learner preferences) | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» «lts_005_1_learner_preferences_writes_are_checked» «lts_005_1_statements_breaking_the_rules_are_refused» |
| 005-2-invalid-au | failed then failed or passed in one session | passes | «lts_005_2_after_failed_neither_failed_nor_passed_is_taken» |
| 006-launchMode | Browse and Review refuse judgements | known deviation: never launched in those modes; the rules for them pass | «known_deviation_every_unit_is_launched_in_normal_mode» «in_browse_or_review_mode_no_satisfaction_is_recorded» |
| 007-1, 007-2 multi-session | completed and passed once per registration, failed never after passed; state and activity id kept across sessions | passes | «lts_007_1_completed_is_taken_once_per_registration» «lts_007_2_passed_is_taken_once_per_registration_and_failed_never_after_it» |
| 008-1-abandoned | abandoned recorded with session, category, duration, actor, object, registration | passes; the late requests fail to connect rather than 401/403 (known deviation) | «lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more» |
| 009-1-waived | waived and its satisfied | known deviation: nothing is ever waived (the suite skips it without `waiveAU`) | «known_deviation_no_unit_is_ever_waived» |
| 101-one-thousand-aus | more than 1000 units import | passes (packaged; the published file names remote units) | «a_course_of_more_than_a_thousand_units_imports» |
| 102-zip64 | a Zip64 archive imports | passes | «a_zip64_package_imports» |
| 201 to 210 | invalid packages are refused | passes, except the schema's element order (known deviation) | see Import |

## Launch (8.1)

| Requirement | Status | Test |
|---|---|---|
| 6.0.0.0-3, 8.1.0.0-4: launched with a URL carrying the launch parameters | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «a_unit_gets_the_cmi5_launch_parameters_and_its_launch_data» |
| 8.1.0.0-1, -3: AnyWindow units play in the player's frame | passes | «a cmi5 unit launches in the frame, runs a session and the package completes» |
| 8.1.0.0-2: OwnWindow units get a window of their own (desktop) | passes | «a_unit_asking_for_its_own_window_gets_one_on_desktop» «a unit that asks for its own window leaves the frame a placeholder that can bring it forward or end it» |
| 8.1.0.0-2 on mobile | known deviation: framed, focus mode forced | «lts_003_an_own_window_unit_gets_its_own_window_on_desktop» |
| 8.1.1.0-1, 8.1.2.0-1, 8.2.1.0-1: endpoint and fetch in the query | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 8.1.3.0-1, 9.2.0.0-1: actor is an Agent with an account, defined by the app | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 8.1.4.0-1, 9.6.1.1-2: registration generated and in the query | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 8.1.5.0-1, -2, -3: a generated activityId, never the publisher id | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «a_unit_gets_the_cmi5_launch_parameters_and_its_launch_data» |
| 8.1.5.0-4: the same activityId on every launch in the registration | passes | «lts_007_1_completed_is_taken_once_per_registration» |
| 13.1.4.0-3: the unit's own query parameters kept, cmi5 parameters after them, before a fragment | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «cmi5_parameters_join_the_units_own_query_before_its_fragment» |

## Fetch URL and token (8.2, 8.1.2)

| Requirement | Status | Test |
|---|---|---|
| 8.2.1.0-2, 8.2.2.0-1, -2, -3: POST answers 200, JSON, `auth-token` | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «the_fetch_url_hands_out_the_token_once» |
| 8.2.3.0-1 (d), 8.2.3.2-1: a second POST gets no token and an error code | passes | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» «the_fetch_url_hands_out_the_token_once» |
| 8.1.2.0-2 (d): no request before the token is fetched | passes | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» |
| 8.1.2.0-2 (d): no request after terminated | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 8.1.2.0-2 (d): no request after abandoned | known deviation: the endpoint is gone, the request fails to connect | «known_deviation_the_endpoint_is_gone_once_the_session_ends» |
| 8.1.2.0-5 (d): a request without the token is refused | passes | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» «the_endpoint_answers_only_its_own_open_session» |
| 6.3.0.0-1: the token cannot void | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_unit_may_not_void_or_use_the_launching_systems_verbs» |

## LaunchData and learner preferences (10, 11)

| Requirement | Status | Test |
|---|---|---|
| 6.0.0.0-2, 10.1.0.0-1 to -5, 9.6.3.1-2: LMS.LaunchData written before launch, JSON, keyed by the launch's activityId, actor and registration, session id in its template | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 10.2.1.0-1, 9.6.2.3-1: contextTemplate with the publisher id in grouping | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 10.2.2.0-4, 10.2.3.0-1, 10.2.4.0-1, 10.2.5.0-1, 10.2.7.0-1, 10.2.7.1-1: launchMode, launchParameters, masteryScore, moveOn, entitlementKey | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 10.2.1.0-5 (d): no PUT, POST or DELETE of LMS.LaunchData | passes | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» |
| 8.1.4.0-3 (d): state requests name the session's registration | passes | «lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded» |
| 10.2.2.0-1 (d), 10.2.2.0-5 (d): the launch mode is tracked and enforced | passes (rules) | «in_browse_or_review_mode_no_satisfaction_is_recorded» |
| 10.2.2.0-2 (d), 10.2.2.0-3 (d): Browse and Review launches | known deviation: never offered | «known_deviation_every_unit_is_launched_in_normal_mode» |
| 10.2.7.2-1: an alternate entitlement key | out of scope: no source agreed with any unit | |
| 6.0.0.0-5, 11.0.0.0-5: the learner preferences document exists with both properties | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 11.0.0.0-1 (d), 8.1.3.0-3 (d): agent profile requests must name the actor (no agent 400, another 403) | passes | «lts_005_1_learner_preferences_writes_are_checked» «the_endpoint_answers_only_its_own_open_session» |
| 11.0.0.0-5 (d), 11.1.0.0-1 (d): writes that are not JSON objects with both properties and a language tag list are refused | passes | «lts_005_1_learner_preferences_writes_are_checked» |
| 11.0.0.0-3 (d2): initialized refused before the preferences are read | known deviation: accepted | «known_deviation_initialized_is_taken_before_the_learner_preferences_are_read» |

## Statements from the unit (7, 9)

| Requirement | Status | Test |
|---|---|---|
| 7.1.1.0-1, 9.3.0.0-4 (d1), (d2): nothing before initialized | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «the_first_statement_of_a_session_must_be_initialized» |
| 9.3.2.0-3 (d), 9.3.0.0-2 (d): a defined verb once per session | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_defined_verb_is_used_once_per_session_and_passed_or_failed_only_once» |
| 9.3.0.0-3 (d): one of passed or failed per session | passes | «lts_005_2_after_failed_neither_failed_nor_passed_is_taken» |
| 9.3.0.0-6, -7, -8: completed and passed once per registration, failed never after passed | passes | «lts_007_1_completed_is_taken_once_per_registration» «lts_007_2_passed_is_taken_once_per_registration_and_failed_never_after_it» «completed_and_passed_are_used_once_per_registration_and_failed_never_follows_passed» |
| 7.1.2.0-1, 7.1.3.0-1, 9.3.0.0-5 (d1), (d2), 9.3.8.0-3: nothing after terminated | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «nothing_is_accepted_after_terminated_or_abandoned» |
| 9.3.8.0-4: a grace period after terminated | known deviation: refused at once | «known_deviation_a_statement_after_terminated_is_refused_at_once» |
| 9.3.6.0-2 (d1), (d2): nothing after abandoned | passes | «nothing_is_accepted_after_terminated_or_abandoned» «lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more» |
| 9.3.0.0-9: allowed statements are recorded and reportable | passes | «lts_002_allowed_statements_are_recorded_and_reported» |
| 9.1.0.0-1 (d), 9.7.0.0-1 (d): no id or no timestamp refused | known deviation: the endpoint fills them in | «known_deviation_a_statement_without_an_id_or_timestamp_gets_one» |
| 9.7.0.0-2 (d): a timestamp not in UTC refused | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_timestamp_must_be_in_utc» |
| 9.2.0.0-2, -3: the actor is the launch's Agent (no Group, no other IFI) | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_statement_outside_the_session_template_is_refused» |
| 8.1.5.0-6 (d), 9.4.0.0-1 (d): a defined statement is about the unit's activityId | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_statement_outside_the_session_template_is_refused» |
| 9.6.0.0-1 (d), 10.2.1.0-6 (d), -7 (d), 9.6.2.0-1 (d): context from the template, its values not overwritten | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_statement_outside_the_session_template_is_refused» |
| 9.6.1.0-1 (d), 8.1.4.0-3 (d): the session's registration | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 9.6.3.1-4 (d): the session id extension | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 9.5.1.0-2 (d): a score only on passed or failed | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «results_follow_the_verb_rules_and_the_mastery_score» |
| 9.5.1.0-3 (d1), (d2): a raw score with min and max | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_raw_score_needs_min_and_max_even_beside_a_scaled_one» |
| 9.5.2.0-1 (d1), 9.5.2.0-2 (d), 9.5.2.0-3 (d): success true on passed, false on failed, absent elsewhere | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 9.5.3.0-1 (d1), 9.5.3.0-2 (d): completion true on completed, absent elsewhere | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 9.5.4.1-1 to -4 (d): duration on terminated, completed, passed, failed | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «results_follow_the_verb_rules_and_the_mastery_score» |
| 9.3.4.0-2, 9.3.5.0-2, 9.5.1.0-1: passed at or above, failed below the mastery score | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «results_follow_the_verb_rules_and_the_mastery_score» |
| 9.6.3.2-2 (d): a scored passed or failed carries the session's mastery score | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «a_scored_judgement_carries_the_mastery_score_it_was_judged_against» |
| 9.6.2.2-1 (d1) to (d3): the moveOn category with success or completion | passes | «lts_005_1_statements_breaking_the_rules_are_refused» |
| 9.6.2.2-2 (d): the moveOn category nowhere else | passes | «lts_005_1_statements_breaking_the_rules_are_refused» «only_a_judgement_or_completion_carries_the_move_on_category» |
| 6.3.0.0-2: statements breaking the rules are voided | known deviation: refused with 403 instead, which 6.3 permits for the AU's credentials; nothing is ever voided | «a_statement_breaking_the_session_rules_is_refused_and_not_stored» |
| A batch is stored whole or not at all | passes | «a_statement_breaking_the_session_rules_is_refused_and_not_stored» |
| The refusal reaches the unit | passes | «a statement the session refuses reaches the unit as a refusal» |

## Statements from the app (9.3, 9.5, 9.6)

| Requirement | Status | Test |
|---|---|---|
| 9.3.1.0-1, -2: launched before the unit runs, once per session | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 9.6.2.1-1, 9.6.3.1-3, 9.6.2.3-2: the app's statements carry the cmi5 category, the session id and the publisher grouping | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more» «the_launching_systems_statements_carry_the_session_context» |
| 9.6.3.2-1, 9.6.3.3-1, 9.6.3.4-1, 9.6.3.6-1, 9.6.3.7-1: launched carries masteryScore, launchMode, the launch URL without cmi5 parameters, moveOn, launchParameters | passes | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 9.3.6.0-1, 9.3.0.0-10: abandoned for a session the unit never terminated, once | passes | «lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more» «a_session_the_unit_never_terminated_is_abandoned_once» «leaving the player mid-session ends the session, which the host records as abandoned» |
| 9.3.6: a new launch, or the next launch after the app stopped, abandons the open session | passes | «launching_again_in_the_registration_abandons_the_open_session» «a_session_left_open_when_the_app_stopped_is_abandoned_at_the_next_launch» |
| 9.5.4.2-1, -2: abandoned carries the session's duration, launch to last statement | passes (present; its value is set by the store from those two times) | «lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more» |
| 9.3.9.0-1 to -9: satisfied for a block and the course, generated object ids, block and course types, the session's id | passes | «lts_001_essentials_launch_launch_data_and_statement_order» «the_package_completes_when_every_unit_meets_its_move_on_criteria» |
| 9.3.3.0-3, 9.3.4.0-4: completed and passed judged against moveOn | passes | «lts_004_1_completed_satisfies_a_completed_unit» «lts_004_3_passed_satisfies_a_passed_unit» «units_satisfy_by_move_on_and_blocks_and_the_course_follow_their_units» |
| 9.6.1.1-3: moveOn evaluated when the registration starts | passes | «lts_004_5_a_not_applicable_unit_is_satisfied_when_the_registration_starts» |
| The package element completes when the course is satisfied | passes | «the_package_completes_when_every_unit_meets_its_move_on_criteria» «a cmi5 unit launches in the frame, runs a session and the package completes» |
| 9.3.7, 9.3.0.0-11, 9.5.2.0-1 (d2), 9.5.3.0-1 (d2), 9.5.5.2-1, 9.6.2.2-1 (d4): waived | known deviation: no unit is ever waived | «known_deviation_no_unit_is_ever_waived» |

## Record store (4.2, 6.0)

| Requirement | Status | Test |
|---|---|---|
| 4.2.0.0-1, 6.0.0.0-1: the endpoint passes the xAPI LRS conformance suite | known deviation: not a conformant record store; statement reads filter but never page, `voidedStatementId` finds nothing | «known_deviation_statement_reads_never_page_and_voided_reads_find_nothing» |
| 4.2.0.0-2: an account that reads every statement across sessions | known deviation: none on the endpoint; the conformance tests read the store in-process by registration, and the learner's records leave through record destinations | «lts_001_essentials_launch_launch_data_and_statement_order» |
| 6.0.0.0-4, 6.2.0.0-1: statement and state requirements of 9 and 10 | passes | rows above |

## Import (3, 6.1, 13, 14)

| Requirement | Status | Test |
|---|---|---|
| 6.0.0.0-6, 6.1.0.0-1, -3: course structure and package import | passes | «a_unit_in_a_block_imports_with_everything_the_course_structure_says_about_it» «the_specifications_complex_example_imports_as_a_tree_once_its_units_are_packaged» |
| 6.1.0.0-2: more than 1000 units (LTS 101) | passes | «a_course_of_more_than_a_thousand_units_imports» |
| 13.1.0.0-1: whitespace trimmed | passes | «a_unit_in_a_block_imports_with_everything_the_course_structure_says_about_it» |
| 3.0.0.0-1 (d): identifiers are absolute IRIs (LTS 201-1 to 201-4) | passes | «an_identifier_that_is_not_an_absolute_iri_is_refused» |
| 14.2.0.0-1 (d), 14.0.0.0-1: a standalone course structure (LTS 202-1 to 202-5, 208) | passes: refused, its units can only be remote | «a_course_structure_without_an_archive_around_it_is_refused» |
| 14.1.0.0-3 (d), -4 (d): relative addresses resolve in the archive, a missing file is refused (LTS 203-1) | passes | «an_address_rooted_at_the_package_resolves_inside_it» «a_unit_whose_file_is_not_in_the_archive_is_refused_as_missing» |
| 8.1.0.0-6 (d): a unit query using a cmi5 parameter name (LTS 204) | passes | «a_unit_url_whose_query_uses_a_cmi5_launch_parameter_name_is_refused» |
| 13.1.2.0-1 (d), 13.1.3.0-1 (d), 13.1.4.0-1 (d): duplicate block, objective, unit ids (LTS 205-1 to 205-3) | passes | «an_identifier_used_twice_is_refused» |
| 13.1.4.0-2 (d): a unit address that is not a URL (LTS 206-1) | passes | «a_unit_address_that_is_not_a_url_is_refused» |
| 13.2.0.0-1 (d), 4.4.0.0-2 (d): values outside the schema, missing required parts | passes | «values_outside_what_the_course_structure_schema_allows_are_refused» «a_course_structure_missing_what_the_standard_requires_is_refused» |
| 13.2.0.0-1 (d): the schema's element order (LTS 207-1) | known deviation: not checked | «known_deviation_element_order_is_not_checked_against_the_schema» |
| 13.1.5.0-1 (d), -2 (d): vendor extensions | known deviation: ignored rather than validated against their schema | «vendor_extensions_are_ignored» |
| 14.1.0.0-1 (d): not a zip (LTS 209-1) | passes | «bytes_that_are_not_a_zip_archive_are_refused» |
| 14.1.0.0-2 (d): no cmi5.xml at the root (LTS 210-1) | passes | «an_archive_with_the_course_structure_one_folder_down_is_refused» |
| 14.0.0.0-1: Zip64 (LTS 102) | passes | «a_zip64_package_imports» |
| 4.4.0.0-1 (d): remote units | out of scope: a package naming one is refused | «one_remote_unit_among_packaged_ones_refuses_the_package» |

## Out of scope

- Remote assignable units and anything loaded from the network (spec: Out of Scope): 8.1.0.0-4 for remote URLs, 14.1.0.0-4 (d) media outside the package, the published 101 and 20x structures as remote courses.
- Course export (13.2.0.0-1 "created by the LMS for export", 14.0.0.0-1 export formats).
- Requirements on the AU (every requirement not naming the LMS), checked only where the launching system must enforce them.

## Not covered here

- The units' own JavaScript is not run. CATAPULT's automated mode (`npx jest` in `lts/` with an LMS adapter) needs a headless host serving the package to a browser, a test-only allowed origin on the endpoint, and a remote read of statements by registration; that crosses the Rust and browser CI jobs and pulls in CATAPULT's webpack and Puppeteer toolchain. The `lts_*` tests replay the same requests and checks instead. The manual procedure (`lts/procedure.md`) on a dev build is the device-run route.
- The desktop own window itself (window chrome, closing it) is a manual check; the player seam checks the placeholder.
- An author's preview of a cmi5 element is not wired: it shows a load error. Not a cmi5 requirement; listed in the ticket notes.
- Device runs (iOS, Android) are manual and recorded in the conformance statement.
