use helix_core::Rope;
use helix_term::typst_preview::{DocumentPreview, FragmentId, Presentation, RenderContext};
use quickcheck::{quickcheck, TestResult};

use super::support::{context, document, ids, rendered};

fn source_for(kind: u8, _index: usize) -> String {
    match kind % 6 {
        0 => format!("$x_{kind}$"),
        1 => format!("$ x_{kind} + y $"),
        2 => format!("```text\nvalue {kind}\n```"),
        3 => format!("#table(columns: 1, [cell {kind}])"),
        4 => format!("#image(\"figure-{kind}.svg\")"),
        _ => format!("#figure(image(\"figure-{kind}.svg\"), caption: [Figure {kind}])"),
    }
}

fn sources(blocks: &[u8]) -> String {
    blocks
        .iter()
        .enumerate()
        .map(|(index, kind)| source_for(*kind, index))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn accept_all(document: &mut DocumentPreview) -> usize {
    let requests = document.requests(context());
    let count = requests.len();
    for request in requests {
        assert!(document.accept_response(request, rendered()));
    }
    count
}

fn is_rendered(document: &mut DocumentPreview, id: FragmentId) -> bool {
    match document
        .presentations(None)
        .and_then(|mut presentations| presentations.find(|(fragment, _)| *fragment == id))
        .map(|(_, presentation)| presentation)
        .expect("fragment presentation")
    {
        Presentation::Rendered { .. } => true,
        Presentation::Source { .. } => false,
    }
}

fn is_source(document: &mut DocumentPreview, id: FragmentId) -> bool {
    !is_rendered(document, id)
}

quickcheck! {
    fn responses_publish_only_when_their_request_is_current(
        original: Vec<u8>,
        replacement: Vec<u8>,
        chosen: usize
    ) -> TestResult {
        if original.is_empty() || original.len() > 6 || replacement.len() > 6 {
            return TestResult::discard();
        }
        let mut document = document(&sources(&original));
        let id = ids(&mut document)[chosen % original.len()];
        let old_request = document
            .requests(context())
            .into_iter()
            .find(|request| request.id() == id)
            .expect("original request");
        document
            .replace_source(Rope::from(sources(&replacement)))
            .expect("replacement must be valid");
        let current = document
            .requests(context())
            .into_iter()
            .find(|request| request.id() == id);
        let expected = current.as_ref() == Some(&old_request);
        let accepted = document.accept_response(old_request, rendered());

        TestResult::from_bool(accepted == expected)
    }

    fn activation_reveals_source_and_deactivation_restores_current_artifacts(
        blocks: Vec<u8>,
        chosen: usize
    ) -> TestResult {
        if blocks.is_empty() || blocks.len() > 6 {
            return TestResult::discard();
        }
        let mut document = document(&sources(&blocks));
        accept_all(&mut document);
        let id = ids(&mut document)[chosen % blocks.len()];
        let active = document
            .presentations(Some(id))
            .and_then(|mut presentations| presentations.find(|(fragment, _)| *fragment == id))
            .map(|(_, presentation)| presentation)
            .expect("active presentation");
        let active_is_source = matches!(active, Presentation::Source { .. });
        let restored = is_rendered(&mut document, id);

        TestResult::from_bool(active_is_source && restored)
    }

    fn context_changes_hide_old_artifacts_until_current_responses(
        blocks: Vec<u8>,
        width_change: bool,
        delta: u8
    ) -> TestResult {
        if blocks.is_empty() || blocks.len() > 6 {
            return TestResult::discard();
        }
        let mut document = document(&sources(&blocks));
        accept_all(&mut document);
        let mut changed: RenderContext = context();
        if width_change {
            changed.available_width_px += f32::from(delta);
        } else {
            let channel = f32::from(delta) / 255.0;
            changed.foreground_rgba = [channel, channel, channel, 1.0];
        }
        let requests = document.requests(changed);
        let old_artifacts_hidden = ids(&mut document)
            .into_iter()
            .all(|id| is_source(&mut document, id));
        accept_all(&mut document);
        let current_artifacts_visible = ids(&mut document)
            .into_iter()
            .all(|id| is_rendered(&mut document, id));

        if width_change && delta == 0 {
            TestResult::from_bool(requests.is_empty() && current_artifacts_visible)
        } else {
            TestResult::from_bool(
                !requests.is_empty() && old_artifacts_hidden && current_artifacts_visible,
            )
        }
    }

    fn unrelated_and_invalid_edits_preserve_current_artifacts(
        blocks: Vec<u8>,
        insertion: usize,
        inserted: u8,
        malformed: bool
    ) -> TestResult {
        if blocks.is_empty() || blocks.len() > 6 {
            return TestResult::discard();
        }
        let mut document = document(&sources(&blocks));
        accept_all(&mut document);
        let original_ids = ids(&mut document);
        let mut edited = blocks.clone();
        let insertion = insertion % (edited.len() + 1);
        edited.insert(insertion, inserted);
        let mut source_parts: Vec<String> = edited
            .iter()
            .enumerate()
            .map(|(index, kind)| source_for(*kind, index))
            .collect();
        source_parts[insertion] = format!("$ inserted_{inserted} $");
        let mut source = source_parts.join("\n\n");
        if malformed {
            source.push_str("\n\n$ unfinished");
        }
        document
            .replace_source(Rope::from(source))
            .expect("edited source must be valid");
        let unchanged_stay_rendered = original_ids
            .into_iter()
            .all(|id| is_rendered(&mut document, id));
        let inserted_id = ids(&mut document)[insertion];
        let inserted_is_source = is_source(&mut document, inserted_id);

        assert!(
            unchanged_stay_rendered && inserted_is_source,
            "unchanged_stay_rendered={unchanged_stay_rendered}, inserted_is_source={inserted_is_source}"
        );
        TestResult::passed()
    }

    fn fragments_accept_responses_independently(blocks: Vec<u8>, chosen: usize) -> TestResult {
        if blocks.is_empty() || blocks.len() > 6 {
            return TestResult::discard();
        }
        let mut document = document(&sources(&blocks));
        let ids = ids(&mut document);
        let chosen = chosen % ids.len();
        let request = document
            .requests(context())
            .into_iter()
            .find(|request| request.id() == ids[chosen])
            .expect("chosen request");
        assert!(document.accept_response(request, rendered()));

        TestResult::from_bool(
            is_rendered(&mut document, ids[chosen])
                && ids
                    .into_iter()
                    .enumerate()
                    .all(|(index, id)| index == chosen || is_source(&mut document, id)),
        )
    }
}
