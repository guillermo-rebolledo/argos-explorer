use std::{fs, io::Write};

use argos_explorer::viewer::{LoadedDocument, load_document, load_page, search_large_file};

#[test]
fn loads_bom_marked_utf16_text() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp.path(), [0xff, 0xfe, b'h', 0, b'i', 0, b'\n', 0]).unwrap();

    let loaded = load_document(temp.path(), 1024).unwrap();
    let LoadedDocument::Text(document) = loaded else {
        panic!("expected text document");
    };
    assert_eq!(document.line(0), Some("hi"));
    assert_eq!(document.encoding.label(), "UTF-16 LE");
}

#[test]
fn binary_content_never_becomes_text() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp.path(), [0, 1, 0, 2, 0, 3]).unwrap();

    let loaded = load_document(temp.path(), 1024).unwrap();
    assert!(matches!(loaded, LoadedDocument::Binary(_)));
}

#[test]
fn large_text_is_paged_and_searchable() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    for index in 0..10_000 {
        writeln!(temp, "line {index}: scalable workspace content").unwrap();
    }
    temp.flush().unwrap();

    let loaded = load_document(temp.path(), 1024).unwrap();
    let LoadedDocument::Large(document) = loaded else {
        panic!("expected paged document");
    };
    let page = load_page(&document, 0, 4096).unwrap();
    assert!(page.text.contains("line 0"));
    assert!(page.next_offset > 0);

    let matches = search_large_file(temp.path(), "line 9999").unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_number, 10_000);
}

#[test]
fn searches_bom_marked_utf16_without_full_file_decoding() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let mut bytes = vec![0xff, 0xfe];
    for unit in "first\nneedle here\nlast\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(temp.path(), bytes).unwrap();

    let matches = search_large_file(temp.path(), "needle").unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_number, 2);
}

#[test]
fn searches_extremely_long_lines_in_bounded_chunks() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    for _ in 0..512 {
        temp.write_all(&vec![b'x'; 4096]).unwrap();
    }
    temp.write_all(b"needle\n").unwrap();
    temp.flush().unwrap();

    let matches = search_large_file(temp.path(), "needle").unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_number, 1);
}
