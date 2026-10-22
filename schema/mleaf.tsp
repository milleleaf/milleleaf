// Milleleaf layer file format (`*.mleaf.json`, and `.mleaf/layers/<hash>.json` / `.mleaf/stacks/<hash>.json`
// inside a revision store).
//
// Written independently from Milleleaf's own code (crates/milleleaf-core/src/model.rs, compose.rs,
// validate.rs, stack.rs; crates/milleleaf-store). The layered model follows the composition ideas of
// Pixar's OpenUSD as adopted by buildingSMART's IFC5: layers stacked bottom-first, later layers
// win, nodes merged by path. No USD or IFC5 schema text is included.
//
// This file describes what the Rust reader accepts today, no more and no less. Rules that only
// `mleaf validate`, the loader or the store enforce are written as comments, not as constraints.
// The JSON member names are fixed: every stored layer is named by the SHA-256 of its canonical
// (RFC 8785) bytes, so renaming a member would change every hash.
//
// Compile: tsp compile schema/ox.tsp --emit @typespec/json-schema

import "@typespec/json-schema";

using TypeSpec.JsonSchema;

@jsonSchema
namespace Milleleaf.Layer;

// Follow-up: unknown members are dropped silently on read and not written back (the Rust structs
// don't deny unknown fields), so no model here sets additionalProperties to false. A misspelt
// member (`attribute` for `attributes`) loses data without an error.

/** One layer: a header, the layers beneath it, attribute schemas and statements about nodes. */
model Layer {
  /** Who made the layer, when, and revision-store metadata. */
  header: Header;

  /** Layers composed beneath this one, loaded depth-first in array order (the store drops them; its stack decides). */
  imports?: Import[];

  /** Attribute key to schema entry; merged across the stack, a later entry replacing an earlier one whole. */
  schemas?: Record<AttributeSchema>;

  /** Statements about nodes, applied in array order and merged by path. */
  data?: Node[];
}

/** Layer header. */
model Header {
  /** Free-text layer id; the provenance label of every attribute the layer sets. Not checked for uniqueness or shape. */
  id: string;

  /** Format version; the code writes `mleaf_alpha`, adapters may leave it empty. Never checked on read. */
  mleafVersion: string;

  /** Version of the content (`1.0.0`, `0.1.0`, `1` seen). Never checked. */
  // Follow-up: milleleaf-web writes `1` while the CLI and store write `1.0.0`; pick one form.
  dataVersion: string;

  /** Free-text author: a person, tool or app. */
  author: string;

  /** Intended as an ISO 8601 date or RFC 3339 date-time; not checked. */
  // Follow-up: no format is enforced on read or by `mleaf validate`; decide whether to require one.
  timestamp: string;

  /** Informational label (`stack`, `edit`, `adopt`, `checkpoint`, `purge`, `base`, `proposal`, `review`, `schema`, ...); open set, only `stack` changes store behaviour. */
  kind?: string | null;

  /** Revision store, stack files: hash (64 hex) of the stack this one replaced. Shape not checked on read. */
  parent?: string | null;

  /** Revision store, ordinary layers: the head (stack hash, 64 hex) the layer was made against. Shape not checked on read. */
  base?: string | null;

  /** Revision store, checkpoints: hashes of the layers it stands for, bottom first. Omitted when empty. */
  replaces?: string[];

  /** Revision store: the content is encrypted; `imports`, `schemas` and `data` are empty. */
  sealed?: Sealed | null;

  /** Revision store, purge records: the layers purged. Omitted when empty. */
  purges?: Purge[];
}

/** How a sealed layer's content is encrypted. All three members are required when `sealed` is present. */
model Sealed {
  /** Encryption scheme; only `hpke-xwing-sha256-aes256gcm/v1` opens. Checked when opening, not on read. */
  scheme: string;

  /** Content-key id, 32 lowercase hex characters; checked when opening, not on read. */
  key: string;

  /** Blob value `sha256-<hex>` of the ciphertext. */
  content: string;
}

/** One purged layer. */
model Purge {
  /** Hash of the purged layer; applied only when exactly 64 hex characters, otherwise ignored. */
  layer: string;

  /** `bytes` (delete the bytes, keep the hash) or `key` (destroy a sealed layer's key); other values are refused when creating and ignored when received. */
  mode: string;

  /** Key name of the purger. */
  by: string;

  /** RFC 3339 time of the purge; format not checked. */
  at: string;

  /** Free-text reason. */
  reason?: string | null;
}

/** A layer composed beneath the importing one. */
model Import {
  /** Path relative to the importing file's directory; any value containing `://` is refused at load time. */
  uri: string;

  /** `sha256-<64 lowercase hex>` of the imported file's exact bytes; a mismatch is a load error. */
  integrity?: string | null;
}

/** One statement about one node; many statements, in many layers, may name the same path. */
// Follow-up: several statements for one path in one layer are allowed and apply in array order.
model Node {
  /** Stable node identifier, in practice UUID-shaped; any string is accepted. Never derived from position. */
  path: string;

  /** Child name to child path; `null` removes a child of that name an earlier statement added. */
  children?: Record<string | null>;

  /** Display order of child names; replaces any earlier order whole. Unlisted children follow, sorted by name. */
  order?: string[] | null;

  /** Name to path of a node whose attributes, children and order this node takes on; `null` removes that inherit. Names apply in ascending order, later winning. */
  // Follow-up: an attribute or child arriving through an inherit can't be removed by `null` on the
  // inheriting node; it has to be overridden or the inherit dropped.
  inherits?: Record<string | null>;

  /** Attribute key (`namespace::name`) to value; a value replaces the earlier one whole, `null` removes it. */
  // The `namespace::name` shape (non-empty part before the first `::`, non-empty remainder) is
  // checked by `mleaf validate`, not on read. Any object inside a value with a string `blob` member
  // names an asset (store: `sha256-<hex>`, or a relative path interned on commit).
  attributes?: Record<unknown>;

  /** Checkpoint layers only: attribute key to the id of the layer that originally set it. */
  provenance?: Record<string>;
}

/** Schema entry for one attribute key (Rust `Schema`). */
model AttributeSchema {
  /** Optional identifier of the definition; never read by the code. */
  // Follow-up: unused anywhere in the repo; keep or drop.
  uri?: string | null;

  /** What the attribute's value must be. */
  // Follow-up: `optional` on this top-level description is accepted but ignored.
  value: ValueDescription;
}

/** The data types a value description can name. */
// What `mleaf validate` accepts for each: Real any number; Integer a number that fits i64 or u64 (a
// number written with a fraction, even `12.0`, fails); Boolean true/false; String any string;
// DateTime any string; Enum a string among the options; Reference a string naming a defined node
// path; Blob a string or an object with a string `blob` member; Array an array; Object an object;
// Any anything. `null` is never a value: it means remove.
// Follow-up: DateTime format is not checked. Integer `12.0` fails in a hand-written file but passes
// once stored (canonical JSON writes `12`). A bare-string Blob passes the validator but the store
// neither interns nor verifies it.
enum DataType {
  Real,
  Integer,
  Boolean,
  String,
  DateTime,
  Enum,
  Array,
  Object,
  Reference,
  Blob,
  Any,
}

/** What a value must be. Descriptions nest without limit. */
// Follow-up: restrictions that don't match the data type are ignored (e.g. enumRestrictions on a
// String), and an Enum without enumRestrictions accepts any string.
model ValueDescription {
  /** The value's data type. */
  dataType: DataType;

  /** Inside an object's member list: the member may be absent. Ignored elsewhere; omitted when false. */
  optional?: boolean;

  /** For Enum: the allowed strings. */
  enumRestrictions?: EnumRestrictions | null;

  /** For Array: length bounds and the item description. */
  arrayRestrictions?: ArrayRestrictions | null;

  /** For Object: the member descriptions. */
  objectRestrictions?: ObjectRestrictions | null;
}

/** Allowed values of an Enum. */
model EnumRestrictions {
  /** The allowed strings. */
  options: string[];
}

/** A non-negative whole number (Rust `usize`; a fraction or a negative number fails to parse). */
@minValue(0)
scalar Length extends integer;

/** Restrictions on an Array. */
model ArrayRestrictions {
  /** Minimum length, inclusive. */
  min?: Length | null;

  /** Maximum length, inclusive. */
  max?: Length | null;

  /** Description every item must match. */
  value: ValueDescription;
}

/** Restrictions on an Object. */
// Validator: a listed member that is present must match; a listed member that isn't optional must
// be present (error); a member not listed gives a warning.
model ObjectRestrictions {
  /** Member name to member description. */
  values: Record<ValueDescription>;
}
