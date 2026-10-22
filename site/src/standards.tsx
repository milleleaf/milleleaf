const standards = [
  {
    name: 'OOXML: ECMA-376 / ISO/IEC 29500',
    status: 'Used and tested',
    use: 'Reads and writes .docx, .xlsx and .pptx through WordCraft, GridCraft and DeckCraft. Writes Transitional. Full Strict support and independent OOXML conformance are not claimed.',
    check: 'hello-table run.sh, steps 6–14: Microsoft Office 16 fixtures, LibreOffice re-saves and edits. These checks cover the fixtures, not every OOXML feature.',
  },
  {
    name: 'ODF: OASIS ODF / ISO/IEC 26300',
    status: 'Used and tested',
    use: 'Reads and writes ODT, ODS and ODP through LibreOffice conversion. No independently validated ODF version or profile is claimed.',
    check: 'hello-table run.sh, step 13: ODF export, LibreOffice edits and import. The format registry reports when LibreOffice is unavailable.',
  },
  {
    name: 'Canonical JSON: RFC 8785',
    status: 'Used and tested',
    use: 'New layers and stacks use JCS canonical bytes for SHA-256 hashing. Numbers must fit I-JSON limits.',
    check: 'ox-store: layers_are_stored_canonical_so_a_resaved_copy_keeps_its_name checks the RFC example and a pretty-printed copy. an_integer_beyond_2_53_is_refused checks the number limit.',
  },
  {
    name: 'Ed25519: RFC 8032',
    status: 'Used and tested',
    use: 'Detached layer signatures. Signing leaves the layer hash unchanged. Trusted project keys and signature policy are checked separately.',
    check: 'ox-store: signatures_are_detached_and_checked checks signing, verification and tampering; hybrid_signatures_old_keys_upgrade_and_policy checks the policy.',
  },
  {
    name: 'ML-DSA-65: FIPS 204',
    status: 'Used and tested',
    use: 'A separate signature beside Ed25519. This is algorithm use, not FIPS module validation. The RustCrypto implementation is unaudited.',
    check: 'ox-store: ml_dsa_65_matches_nist_acvp_and_wycheproof checks pinned NIST ACVP and Wycheproof subsets.',
  },
  {
    name: 'SLH-DSA: FIPS 205',
    status: 'Used and tested',
    use: 'SLH-DSA-SHA2-192s adds a separate signature for owners, approvers and archive signing. No FIPS module validation is claimed.',
    check: 'ox-store: slh_dsa_matches_nist_acvp and slh_dsa_fips205_and_rustcrypto_verify_each_other check vectors and two implementations.',
  },
  {
    name: 'HPKE: RFC 9180, with X-Wing',
    status: 'Used and tested',
    use: 'Sealing uses HPKE base mode, X-Wing, HKDF-SHA256 and AES-256-GCM. X-Wing is a draft hybrid KEM, not a final RFC-standard KEM.',
    check: 'ox-seal: boringssl_seals_and_rustcrypto_opens_and_the_reverse and both_implementations_match_the_hpke_pq_test_vector. The published draft vector uses ChaCha20-Poly1305; AES-256-GCM is checked by interoperation, not a published vector.',
  },
  {
    name: 'Time-stamps: RFC 3161',
    status: 'Used and tested',
    use: 'Stores TSA tokens over signature files. Rust checks the imprint and time only. It does not check the TSA signature or certificate chain. The user chooses the TSA.',
    check: 'ox-store: a_time_stamp_token_is_checked_against_its_signature_file. hello-table run.sh step 15 checks a local test TSA with openssl ts -verify, including its signature and certificate.',
  },
  {
    name: 'OpenTimestamps',
    status: 'Used and tested',
    use: 'Writes .ots proofs and upgrades through chosen calendars. Pending proofs stay labelled pending. Offline verification does not claim Bitcoin confirmation; no qualified time-stamp claim.',
    check: 'hello-table run.sh step 15 uses a local test calendar for pending proofs. ox-store: a_real_confirmed_proof_checks_offline checks a recorded proof against a recorded header. Online header checks alone do not prove best-chain membership.',
  },
  {
    name: 'DocLang 0.8',
    status: 'Used and tested',
    use: 'Exports .dclg and .dclx for AI tools. It is an export, not a substitute for the signed source.',
    check: 'hello-table run.sh step 19 runs the hash-pinned doclang 0.8.0 validator on both exports and checks that it rejects a broken table.',
  },
  {
    name: 'WCAG 2.2 AA',
    status: 'Not claimed',
    use: 'This site targets WCAG 2.2 AA. Conformance is not claimed.',
    check: 'site/scripts/check.mjs checks keyboard skip-link focus, overflow, both colour themes and reduced motion. These checks are partial; a full accessibility assessment is still needed.',
  },
  {
    name: 'PDF/A-3b and PDF/UA',
    status: 'Planned',
    use: 'M3.5 plans independently validated archival and accessible PDF output. Current packs are plain PDF. Neither PDF/A nor PDF/UA is claimed.',
    check: 'Planned: veraPDF checks and a conformance statement. Slide pages need real text structure before an accessibility claim.',
  },
  {
    name: 'ISO 19650 workflow',
    status: 'Planned',
    use: 'M3 plans support for the ISO 19650 workflow: states, coded IDs, status, revision and two-step publication, alongside the existing CDE.',
    check: 'No workflow conformance claim. Planned support does not replace CDE security handling or assessed certification.',
  },
]

export function StandardsTable() {
  return <>
    <section aria-labelledby="standards-register">
      <h2 id="standards-register">What we use. What we have checked.</h2>
      <p>Used and tested means the named checks cover that use. It does not mean the product or every output has independent standards validation.</p>
      <div className="standards-table-container">
        <table className="standards-table" aria-labelledby="standards-register">
          <thead><tr>{['Standard', 'Status', 'Use and limits', 'Test or check'].map(label => <th key={label} scope="col">{label}</th>)}</tr></thead>
          <tbody>{standards.map(item => <tr key={item.name}>
            <th scope="row" data-label="Standard">{item.name}</th>
            <td data-label="Status"><strong>{item.status}</strong></td>
            <td data-label="Use and limits">{item.use}</td>
            <td data-label="Test or check">{item.check}</td>
          </tr>)}</tbody>
        </table>
      </div>
    </section>
    <section className="page-leaf" aria-labelledby="identity-markers">
      <h2 id="identity-markers">Identity in the places the format allows.</h2>
      <div><p>Milleleaf uses content-control tags in Word, defined names in workbooks, shape names in decks and custom file properties. ECMA-376 defines these places. The files open normally in the checked apps.</p>
        <p>The standard allows these markers. It does not promise an app will keep them, or that copying a table or slide will give it a new identity. Milleleaf checks survival per app, reports duplicate IDs and treats a content lock as a hint. A typed-over link is checked again on import.</p>
        <p>Tests are named above so each claim has a boundary.</p>
      </div>
    </section>
  </>
}
