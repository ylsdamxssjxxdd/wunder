type Dict = Record<string, unknown>;

const toText = (value: unknown): string => {
  if (typeof value !== 'string') return '';
  return value.trim();
};

const toObject = (value: unknown): Dict | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as Dict;
};

const toStringArray = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => toText(item))
    .filter((item) => item.length > 0);
};

const resolveSchema = (tool: Dict): Dict | null =>
  toObject(tool.input_schema) || toObject(tool.inputSchema) || toObject(tool.schema);

const resolveActionValues = (schema: Dict): string[] => {
  const properties = toObject(schema.properties);
  const action = toObject(properties?.action);
  return toStringArray(action?.enum);
};

const resolveRequiredFields = (schema: Dict): string[] => toStringArray(schema.required);

const resolveArgNames = (schema: Dict): string[] => {
  const properties = toObject(schema.properties);
  if (!properties) return [];
  return Object.keys(properties).filter((name) => toText(name).length > 0).slice(0, 4);
};

export const resolveToolUsageHint = (toolLike: unknown): string => {
  const tool = toObject(toolLike);
  if (!tool) return '';
  const label = toText(tool.label) || toText(tool.name);
  const description = toText(tool.description);
  if (description) return description;
  const schema = resolveSchema(tool);
  if (!schema) return label;

  const parts: string[] = [];
  const actionValues = resolveActionValues(schema);
  if (actionValues.length) {
    parts.push(`action: ${actionValues.join('/')}`);
  }
  const requiredFields = resolveRequiredFields(schema);
  if (requiredFields.length) {
    parts.push(`required: ${requiredFields.join(', ')}`);
  }
  if (!parts.length) {
    const argNames = resolveArgNames(schema);
    if (argNames.length) {
      parts.push(`args: ${argNames.join(', ')}`);
    }
  }
  return parts.join(' | ') || label;
};
