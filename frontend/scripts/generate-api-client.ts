import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

type OpenApiSchema = {
  type?: string | string[];
  format?: string;
  nullable?: boolean;
  enum?: string[];
  items?: OpenApiSchema;
  additionalProperties?: boolean | OpenApiSchema;
  properties?: Record<string, OpenApiSchema>;
  required?: string[];
  $ref?: string;
  allOf?: OpenApiSchema[];
  anyOf?: OpenApiSchema[];
  oneOf?: OpenApiSchema[];
};

type OpenApiDocument = {
  components?: {
    schemas?: Record<string, OpenApiSchema>;
  };
};

const apiBaseUrl = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";
const outputPath = resolve(process.cwd(), "src/lib/generated/api-types.ts");

const response = await fetch(`${apiBaseUrl.replace(/\/+$/, "")}/api/v1/openapi.json`);
if (!response.ok) {
  throw new Error(`Failed to fetch OpenAPI document: ${response.status}`);
}

const document = (await response.json()) as OpenApiDocument;
const schemas = document.components?.schemas ?? {};
const lines = [
  "// Generated from /api/v1/openapi.json. Do not edit by hand.",
  "",
  "export type JsonPrimitive = string | number | boolean | null;",
  "export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };",
  "export type Uuid = string;",
  "export type DateTime = string;",
  "",
];

for (const [name, schema] of Object.entries(schemas).sort(([left], [right]) => left.localeCompare(right))) {
  lines.push(`export type ${name} = ${schemaToType(schema)};`, "");
}

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${lines.join("\n").trimEnd()}\n`);

function schemaToType(schema: OpenApiSchema): string {
  const types = schemaTypes(schema);
  if (types.includes("null")) {
    const nonNullTypes = types.filter((type) => type !== "null");
    if (nonNullTypes.length === 0) {
      return "null";
    }
    const nonNullSchema = {
      ...schema,
      type: nonNullTypes.length === 1 ? nonNullTypes[0] : nonNullTypes,
      nullable: false,
    };
    return `${schemaToNonNullableType(nonNullSchema)} | null`;
  }
  const type = schemaToNonNullableType(schema);
  return schema.nullable ? `${type} | null` : type;
}

function schemaToNonNullableType(schema: OpenApiSchema): string {
  if (schema.$ref) {
    return refName(schema.$ref);
  }
  if (schema.allOf?.length) {
    return schema.allOf.map(schemaToType).join(" & ");
  }
  if (schema.anyOf?.length || schema.oneOf?.length) {
    const variants = schema.anyOf ?? schema.oneOf ?? [];
    return variants.map(schemaToType).join(" | ");
  }
  if (schema.enum) {
    return schema.enum.map((value) => JSON.stringify(value)).join(" | ");
  }
  if (hasSchemaType(schema, "array")) {
    const itemType = schemaToType(schema.items ?? {});
    return `${arrayItemType(itemType)}[]`;
  }
  if (hasSchemaType(schema, "null")) {
    return "null";
  }
  if (hasSchemaType(schema, "object") || schema.properties) {
    const properties = schema.properties ?? {};
    const required = new Set(schema.required ?? []);
    const entries = Object.entries(properties).map(([key, value]) => {
      const optional = required.has(key) ? "" : "?";
      return `  ${JSON.stringify(key)}${optional}: ${schemaToType(value)};`;
    });
    if (entries.length === 0 && schema.additionalProperties) {
      const valueType =
        typeof schema.additionalProperties === "object"
          ? schemaToType(schema.additionalProperties)
          : "JsonValue";
      return `{ [key: string]: ${valueType} }`;
    }
    return `{\n${entries.join("\n")}\n}`;
  }
  const base =
    schema.format === "uuid"
      ? "Uuid"
      : schema.format === "date-time"
        ? "DateTime"
        : hasSchemaType(schema, "integer") || hasSchemaType(schema, "number")
          ? "number"
          : hasSchemaType(schema, "boolean")
            ? "boolean"
            : hasSchemaType(schema, "string")
              ? "string"
              : "JsonValue";
  return base;
}

function schemaTypes(schema: OpenApiSchema): string[] {
  if (Array.isArray(schema.type)) {
    return schema.type;
  }
  return schema.type ? [schema.type] : [];
}

function hasSchemaType(schema: OpenApiSchema, type: string): boolean {
  return schemaTypes(schema).includes(type);
}

function arrayItemType(type: string): string {
  if (type.includes(" | ") || type.includes(" & ")) {
    return `(${type})`;
  }
  return type;
}

function refName(ref: string): string {
  const name = ref.split("/").pop();
  if (!name) {
    throw new Error(`Invalid OpenAPI ref: ${ref}`);
  }
  return name;
}
