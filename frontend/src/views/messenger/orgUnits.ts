import { UNIT_UNGROUPED_ID, type UnitTreeNode, type UnitTreeRow } from '@/views/messenger/model';

export const normalizeUnitText = (value: unknown): string => String(value || '').trim();

export const resolveUnitIdKey = (unitId: unknown): string => {
  const cleaned = normalizeUnitText(unitId);
  return cleaned || UNIT_UNGROUPED_ID;
};

export const normalizeUnitShortLabel = (value: unknown): string => {
  const text = normalizeUnitText(value);
  if (!text) return '';
  const normalized = text
    .replace(/->/g, '/')
    .replace(/>/g, '/')
    .replace(/\\/g, '/')
    .replace(/\|/g, '/');
  const parts = normalized
    .split('/')
    .map((item) => item.trim())
    .filter(Boolean);
  if (parts.length > 1) {
    return parts[parts.length - 1];
  }
  return text;
};

export const normalizeUnitNode = (value: unknown): UnitTreeNode | null => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  const unitId = normalizeUnitText(source.unit_id || source.id);
  if (!unitId) return null;
  const parentId = normalizeUnitText(source.parent_id || source.parentId);
  const sortOrder = Number(source.sort_order ?? source.sortOrder);
  const label = normalizeUnitShortLabel(
    source.name ||
      source.unit_name ||
      source.unitName ||
      source.display_name ||
      source.displayName ||
      source.path_name ||
      source.pathName
  );
  const children = (Array.isArray(source.children) ? source.children : [])
    .map((item) => normalizeUnitNode(item))
    .filter((item): item is UnitTreeNode => Boolean(item));
  const hydratedChildren = children.map((child) => ({
    ...child,
    parentId: child.parentId || unitId
  }));
  return {
    id: unitId,
    label: label || unitId,
    parentId,
    sortOrder: Number.isFinite(sortOrder) ? sortOrder : 0,
    children: hydratedChildren
  };
};

export const flattenUnitNodes = (nodes: UnitTreeNode[], sink: UnitTreeNode[] = []): UnitTreeNode[] => {
  nodes.forEach((node) => {
    sink.push({
      id: node.id,
      label: node.label,
      parentId: node.parentId,
      sortOrder: node.sortOrder,
      children: []
    });
    if (node.children.length) {
      flattenUnitNodes(node.children, sink);
    }
  });
  return sink;
};

export const buildUnitTreeFromFlat = (nodes: UnitTreeNode[]): UnitTreeNode[] => {
  const nodeMap = new Map<string, UnitTreeNode>();
  nodes.forEach((node) => {
    const id = normalizeUnitText(node.id);
    if (!id) return;
    const existing = nodeMap.get(id);
    if (existing) {
      if (!existing.label || existing.label === existing.id) {
        existing.label = node.label || id;
      }
      if (!existing.parentId && node.parentId) {
        existing.parentId = node.parentId;
      }
      if ((!Number.isFinite(existing.sortOrder) || existing.sortOrder === 0) && Number.isFinite(node.sortOrder)) {
        existing.sortOrder = node.sortOrder;
      }
      return;
    }
    nodeMap.set(id, {
      id,
      label: node.label || id,
      parentId: normalizeUnitText(node.parentId),
      sortOrder: Number.isFinite(node.sortOrder) ? node.sortOrder : 0,
      children: []
    });
  });

  const hasAncestor = (node: UnitTreeNode, ancestorId: string): boolean => {
    let cursor = normalizeUnitText(node.parentId);
    let guard = 0;
    while (cursor && guard < nodeMap.size) {
      if (cursor === ancestorId) {
        return true;
      }
      const parent = nodeMap.get(cursor);
      if (!parent) {
        break;
      }
      cursor = normalizeUnitText(parent.parentId);
      guard += 1;
    }
    return false;
  };

  const roots: UnitTreeNode[] = [];
  nodeMap.forEach((node) => {
    const parentId = normalizeUnitText(node.parentId);
    const parent = parentId ? nodeMap.get(parentId) : null;
    if (!parent || parent.id === node.id || hasAncestor(parent, node.id)) {
      roots.push(node);
      return;
    }
    parent.children.push(node);
  });

  const sortNodes = (list: UnitTreeNode[]) => {
    list.sort((left, right) => {
      const leftOrder = Number.isFinite(left.sortOrder) ? left.sortOrder : 0;
      const rightOrder = Number.isFinite(right.sortOrder) ? right.sortOrder : 0;
      if (leftOrder !== rightOrder) return leftOrder - rightOrder;
      return left.label.localeCompare(right.label, 'zh-CN');
    });
    list.forEach((node) => sortNodes(node.children));
  };
  sortNodes(roots);
  return roots;
};

export const collectUnitNodeIds = (nodes: UnitTreeNode[], sink: Set<string>) => {
  nodes.forEach((node) => {
    sink.add(node.id);
    if (node.children.length) {
      collectUnitNodeIds(node.children, sink);
    }
  });
};

export const buildUnitTreeRows = (
  nodes: UnitTreeNode[],
  depth: number,
  directCountMap: Map<string, number>,
  isExpanded: (unitId: string) => boolean
): { rows: UnitTreeRow[]; total: number } => {
  let rows: UnitTreeRow[] = [];
  let total = 0;
  nodes.forEach((node) => {
    const child = buildUnitTreeRows(node.children, depth + 1, directCountMap, isExpanded);
    const count = (directCountMap.get(node.id) || 0) + child.total;
    if (count <= 0) {
      return;
    }
    const hasChildren = child.rows.length > 0;
    const expanded = hasChildren && isExpanded(node.id);
    rows.push({
      id: node.id,
      label: node.label,
      depth,
      count,
      hasChildren,
      expanded
    });
    if (expanded) {
      rows = rows.concat(child.rows);
    }
    total += count;
  });
  return { rows, total };
};

export const resolveUnitTreeRowStyle = (row: UnitTreeRow): Record<string, string> => ({
  '--messenger-unit-depth': String(Math.max(0, row.depth))
});
