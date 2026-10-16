// OX layer file format (alpha), derived from IFC5's ifcx.tsp (buildingSMART/IFC5-development).
// Differences from IFCX: `order` on nodes, `kind` in the header, and the `Any` data type; and for
// the revision store: `parent`, `base` and `replaces` in the header and
// `provenance` on checkpoint nodes.
// Attribute schemas for office content live in office.ox.json, as data, like IFCX.

@pattern("^[A-Za-z0-9_\\-/.:]+$")
scalar path extends string;

model OxNode {
    path: path;
    /** child name → child path; null removes a child an earlier layer added */
    children?: Record<path | null>;
    /** OX: order of child names; replaces any earlier order */
    order?: string[];
    /** name → path of a node whose attributes and children this node takes on */
    inherits?: Record<path | null>;
    /** `namespace::name` → value; null removes the attribute */
    attributes?: Record<unknown>;
    /** OX, checkpoints only: attribute → id of the layer that originally set it */
    provenance?: Record<string>;
}

model OxHeader {
    id: string;
    oxVersion: "ox_alpha";
    dataVersion: string;
    author: string;
    timestamp: string;
    /** OX: schema, base, proposal, review, edit, stack, checkpoint … (informational) */
    kind?: string;
    /** OX store, on a stack: hash of the stack it replaced (history) */
    parent?: string;
    /** OX store, on a layer: hash of the head it was made against (compare-and-swap) */
    base?: string;
    /** OX store, on a checkpoint: hashes of the layers it stands for, bottom first */
    replaces?: string[];
}

enum DataType {
    Real: "Real",
    Boolean: "Boolean",
    Integer: "Integer",
    String: "String",
    DateTime: "DateTime",
    Enum: "Enum",
    Array: "Array",
    Object: "Object",
    Reference: "Reference",
    Blob: "Blob",
    /** OX: any JSON value, unchecked */
    Any: "Any",
}

model EnumRestrictions {
    options: string[];
}

model ArrayRestrictions {
    min?: int32;
    max?: int32;
    value: OxValueDescription;
}

model ObjectRestrictions {
    values: Record<OxValueDescription>;
}

model OxValueDescription {
    dataType: DataType;
    optional?: boolean;
    enumRestrictions?: EnumRestrictions;
    arrayRestrictions?: ArrayRestrictions;
    objectRestrictions?: ObjectRestrictions;
}

model OxSchema {
    uri?: string;
    value: OxValueDescription;
}

model ImportNode {
    uri: string;
    /** sha256-<hex> of the imported file */
    integrity?: string;
}

model OxFile {
    header: OxHeader;
    imports: ImportNode[];
    schemas: Record<OxSchema>;
    data: OxNode[];
}
