import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import ts from 'typescript';

const sourcePath = path.resolve('frontend/src/views/MessengerView.vue');
const messengerDir = path.resolve('frontend/src/views/messenger');
const controllerDir = path.join(messengerDir, 'controller');
const targetBodyLines = 1650;
const controllerModules = [
  {
    file: 'messengerControllerCoreState.ts',
    installer: 'installMessengerControllerCoreState',
    description: 'Core state, stores, shell layout, active agent identity, model display, and approval state.'
  },
  {
    file: 'messengerControllerNavigationLists.ts',
    installer: 'installMessengerControllerNavigationLists',
    description: 'Navigation lists, right-dock skills, file containers, contacts, swarms, tools, and mixed conversation ordering.'
  },
  {
    file: 'messengerControllerConversationRuntime.ts',
    installer: 'installMessengerControllerConversationRuntime',
    description: 'Conversation runtime helpers for agent unread state, world drafts, composer resizing, voice, rendering, and plans.'
  },
  {
    file: 'messengerControllerMessageResources.ts',
    installer: 'installMessengerControllerMessageResources',
    description: 'Message resource hydration, workspace attachments, image preview, history helpers, and persisted ordering utilities.'
  },
  {
    file: 'messengerControllerWorkspaceActions.ts',
    installer: 'installMessengerControllerWorkspaceActions',
    description: 'User actions for ordering, middle-pane selection, agent creation/import/export, timeline, and file container menus.'
  },
  {
    file: 'messengerControllerMessagingSettings.ts',
    installer: 'installMessengerControllerMessagingSettings',
    description: 'Messaging commands, world uploads, helper apps, new sessions, profile/settings updates, and approval decisions.'
  },
  {
    file: 'messengerControllerLifecycle.ts',
    installer: 'installMessengerControllerLifecycle',
    description: 'Runtime metadata loading, realtime pulse, message viewport, route restore, watchers, mount, and cleanup lifecycle.'
  }
];

const source = (
  process.argv.includes('--from-head')
    ? execFileSync('git', ['show', 'HEAD:frontend/src/views/MessengerView.vue'], { encoding: 'utf8' })
    : fs.readFileSync(sourcePath, 'utf8')
).replace(/^\uFEFF/, '');
const scriptMatch = source.match(/<script setup[^>]*>([\s\S]*?)<\/script>/);
const rootTemplateStart = source.indexOf('<template>');
const rootTemplateEnd = scriptMatch ? source.lastIndexOf('</template>', scriptMatch.index) : -1;
const templateBlock =
  rootTemplateStart >= 0 && rootTemplateEnd >= rootTemplateStart
    ? source.slice(rootTemplateStart, rootTemplateEnd + '</template>'.length).trimEnd()
    : '';

if (!templateBlock || !scriptMatch) {
  throw new Error('MessengerView.vue does not match the expected single-file component shape.');
}

const script = scriptMatch[1];
const sourceFile = ts.createSourceFile('MessengerView.script.ts', script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed, removeComments: false });
const factory = ts.factory;

function textOf(node) {
  return node.getText(sourceFile);
}

function printNode(node) {
  return printer.printNode(ts.EmitHint.Unspecified, node, sourceFile);
}

function lineCount(text) {
  return text.split(/\r?\n/).length;
}

function collectBindingNames(name, output = []) {
  if (!name) return output;
  if (ts.isIdentifier(name)) {
    output.push(name.text);
    return output;
  }
  if (ts.isObjectBindingPattern(name) || ts.isArrayBindingPattern(name)) {
    for (const element of name.elements) {
      if (ts.isBindingElement(element)) {
        collectBindingNames(element.name, output);
      }
    }
  }
  return output;
}

function collectTopNames() {
  const names = [];
  for (const statement of sourceFile.statements) {
    if (ts.isVariableStatement(statement)) {
      for (const declaration of statement.declarationList.declarations) {
        collectBindingNames(declaration.name, names);
      }
    } else if (ts.isFunctionDeclaration(statement) && statement.name) {
      names.push(statement.name.text);
    }
  }
  return [...new Set(names)];
}

function collectImportValueNames() {
  const names = [];
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement)) continue;
    const clause = statement.importClause;
    if (!clause || clause.isTypeOnly) continue;
    if (clause.name) names.push(clause.name.text);
    const bindings = clause.namedBindings;
    if (bindings && ts.isNamespaceImport(bindings)) names.push(bindings.name.text);
    if (bindings && ts.isNamedImports(bindings)) {
      for (const element of bindings.elements) {
        if (!element.isTypeOnly) names.push(element.name.text);
      }
    }
  }
  return [...new Set(names)];
}

const topNames = collectTopNames();
const topNameSet = new Set(topNames);
const importValueNames = collectImportValueNames();
const importsText = sourceFile.statements.filter(ts.isImportDeclaration).map(textOf).join('\n');
const typeAliasText = sourceFile.statements.filter(ts.isTypeAliasDeclaration).map(textOf).join('\n\n');

function blockDirectLocals(block) {
  const names = [];
  for (const statement of block.statements || []) {
    if (ts.isVariableStatement(statement)) {
      for (const declaration of statement.declarationList.declarations) {
        collectBindingNames(declaration.name, names);
      }
    } else if ((ts.isFunctionDeclaration(statement) || ts.isClassDeclaration(statement)) && statement.name) {
      names.push(statement.name.text);
    }
  }
  return new Set(names);
}

function variableDeclarationListNames(list) {
  const names = [];
  if (!list) return new Set(names);
  for (const declaration of list.declarations || []) {
    collectBindingNames(declaration.name, names);
  }
  return new Set(names);
}

function parameterNames(parameters) {
  const names = [];
  for (const parameter of parameters || []) {
    collectBindingNames(parameter.name, names);
  }
  return new Set(names);
}

function hasLocal(name, scopes) {
  for (let index = scopes.length - 1; index >= 0; index -= 1) {
    if (scopes[index].has(name)) return true;
  }
  return false;
}

function contextAccess(name) {
  return factory.createPropertyAccessExpression(factory.createIdentifier('ctx'), name);
}

function transformWithScopes(inputNode, initialScopes = []) {
  const result = ts.transform(inputNode, [
    (context) => {
      const visit = (node, scopes) => {
        if (!node) return node;
        if (
          ts.isTypeNode(node) ||
          ts.isTypeElement(node) ||
          ts.isTypeAliasDeclaration(node) ||
          ts.isInterfaceDeclaration(node) ||
          ts.isImportDeclaration(node)
        ) {
          return node;
        }
        if (ts.isIdentifier(node)) {
          if (topNameSet.has(node.text) && !hasLocal(node.text, scopes)) {
            return contextAccess(node.text);
          }
          return node;
        }
        if (ts.isPropertyAccessExpression(node)) {
          return factory.updatePropertyAccessExpression(node, visit(node.expression, scopes), node.name);
        }
        if (ts.isPropertyAssignment(node)) {
          const nextName = ts.isComputedPropertyName(node.name) ? visit(node.name, scopes) : node.name;
          return factory.updatePropertyAssignment(node, nextName, visit(node.initializer, scopes));
        }
        if (ts.isShorthandPropertyAssignment(node)) {
          const name = node.name.text;
          if (topNameSet.has(name) && !hasLocal(name, scopes)) {
            return factory.createPropertyAssignment(name, contextAccess(name));
          }
          return node;
        }
        if (ts.isBindingElement(node)) {
          const propertyName =
            node.propertyName && ts.isComputedPropertyName(node.propertyName)
              ? visit(node.propertyName, scopes)
              : node.propertyName;
          return factory.updateBindingElement(
            node,
            node.dotDotDotToken,
            propertyName,
            node.name,
            node.initializer ? visit(node.initializer, scopes) : undefined
          );
        }
        if (ts.isVariableDeclaration(node)) {
          return factory.updateVariableDeclaration(
            node,
            node.name,
            node.exclamationToken,
            node.type,
            node.initializer ? visit(node.initializer, scopes) : undefined
          );
        }
        if (ts.isParameter(node)) {
          return factory.updateParameterDeclaration(
            node,
            node.modifiers,
            node.dotDotDotToken,
            node.name,
            node.questionToken,
            node.type,
            node.initializer ? visit(node.initializer, scopes) : undefined
          );
        }
        if (ts.isBlock(node)) {
          const locals = blockDirectLocals(node);
          const nextScopes = locals.size ? scopes.concat(locals) : scopes;
          return factory.updateBlock(
            node,
            node.statements.map((statement) => visit(statement, nextScopes))
          );
        }
        if (ts.isFunctionDeclaration(node)) {
          const locals = parameterNames(node.parameters);
          if (node.name) locals.add(node.name.text);
          const nextScopes = scopes.concat(locals);
          return factory.updateFunctionDeclaration(
            node,
            node.modifiers,
            node.asteriskToken,
            node.name,
            node.typeParameters,
            node.parameters.map((parameter) => visit(parameter, nextScopes)),
            node.type,
            node.body ? visit(node.body, nextScopes) : node.body
          );
        }
        if (ts.isFunctionExpression(node)) {
          const locals = parameterNames(node.parameters);
          if (node.name) locals.add(node.name.text);
          const nextScopes = scopes.concat(locals);
          return factory.updateFunctionExpression(
            node,
            node.modifiers,
            node.asteriskToken,
            node.name,
            node.typeParameters,
            node.parameters.map((parameter) => visit(parameter, nextScopes)),
            node.type,
            node.body ? visit(node.body, nextScopes) : node.body
          );
        }
        if (ts.isArrowFunction(node)) {
          const locals = parameterNames(node.parameters);
          const nextScopes = scopes.concat(locals);
          return factory.updateArrowFunction(
            node,
            node.modifiers,
            node.typeParameters,
            node.parameters.map((parameter) => visit(parameter, nextScopes)),
            node.type,
            node.equalsGreaterThanToken,
            visit(node.body, nextScopes)
          );
        }
        if (ts.isMethodDeclaration(node)) {
          const locals = parameterNames(node.parameters);
          const nextScopes = scopes.concat(locals);
          const name = ts.isComputedPropertyName(node.name) ? visit(node.name, scopes) : node.name;
          return factory.updateMethodDeclaration(
            node,
            node.modifiers,
            node.asteriskToken,
            name,
            node.questionToken,
            node.typeParameters,
            node.parameters.map((parameter) => visit(parameter, nextScopes)),
            node.type,
            node.body ? visit(node.body, nextScopes) : node.body
          );
        }
        if (ts.isForStatement(node)) {
          const locals =
            node.initializer && ts.isVariableDeclarationList(node.initializer)
              ? variableDeclarationListNames(node.initializer)
              : new Set();
          const nextScopes = locals.size ? scopes.concat(locals) : scopes;
          return factory.updateForStatement(
            node,
            node.initializer ? visit(node.initializer, nextScopes) : undefined,
            node.condition ? visit(node.condition, nextScopes) : undefined,
            node.incrementor ? visit(node.incrementor, nextScopes) : undefined,
            visit(node.statement, nextScopes)
          );
        }
        if (ts.isForOfStatement(node)) {
          const locals = ts.isVariableDeclarationList(node.initializer)
            ? variableDeclarationListNames(node.initializer)
            : new Set();
          const nextScopes = locals.size ? scopes.concat(locals) : scopes;
          return factory.updateForOfStatement(
            node,
            node.awaitModifier,
            visit(node.initializer, nextScopes),
            visit(node.expression, scopes),
            visit(node.statement, nextScopes)
          );
        }
        if (ts.isForInStatement(node)) {
          const locals = ts.isVariableDeclarationList(node.initializer)
            ? variableDeclarationListNames(node.initializer)
            : new Set();
          const nextScopes = locals.size ? scopes.concat(locals) : scopes;
          return factory.updateForInStatement(
            node,
            visit(node.initializer, nextScopes),
            visit(node.expression, scopes),
            visit(node.statement, nextScopes)
          );
        }
        if (ts.isCatchClause(node)) {
          const locals = new Set();
          if (node.variableDeclaration) {
            for (const name of collectBindingNames(node.variableDeclaration.name, [])) {
              locals.add(name);
            }
          }
          const nextScopes = locals.size ? scopes.concat(locals) : scopes;
          return factory.updateCatchClause(
            node,
            node.variableDeclaration ? visit(node.variableDeclaration, nextScopes) : undefined,
            visit(node.block, nextScopes)
          );
        }
        return ts.visitEachChild(node, (child) => visit(child, scopes), context);
      };

      return (root) => visit(root, initialScopes);
    }
  ]);
  const transformed = result.transformed[0];
  result.dispose();
  return transformed;
}

function transformExpression(expression) {
  return transformWithScopes(expression, []);
}

function bindingAssignmentStatements(name) {
  return collectBindingNames(name).map((bindingName) => `ctx.${bindingName} = ${bindingName};`);
}

function declarationKeyword(statement) {
  return statement.declarationList.flags & ts.NodeFlags.Const ? 'const' : 'let';
}

function transformVariableStatement(statement) {
  const lines = [];
  for (const declaration of statement.declarationList.declarations) {
    if (ts.isIdentifier(declaration.name)) {
      const initializer = declaration.initializer ? printNode(transformExpression(declaration.initializer)) : 'undefined';
      lines.push(`ctx.${declaration.name.text} = ${initializer};`);
      continue;
    }
    const initializer = declaration.initializer ? printNode(transformExpression(declaration.initializer)) : 'undefined';
    lines.push(`${declarationKeyword(statement)} ${textOf(declaration.name)} = ${initializer};`);
    lines.push(...bindingAssignmentStatements(declaration.name));
  }
  return lines.join('\n');
}

function transformFunctionDeclaration(statement) {
  const locals = parameterNames(statement.parameters);
  if (statement.name) locals.add(statement.name.text);
  const body = statement.body ? transformWithScopes(statement.body, [locals]) : statement.body;
  const parameters = statement.parameters.map((parameter) => transformWithScopes(parameter, [locals]));
  const expression = factory.createFunctionExpression(
    statement.modifiers,
    statement.asteriskToken,
    statement.name,
    statement.typeParameters,
    parameters,
    statement.type,
    body
  );
  return printNode(factory.createExpressionStatement(factory.createAssignment(contextAccess(statement.name.text), expression)));
}

function transformStatement(statement) {
  if (ts.isVariableStatement(statement)) return transformVariableStatement(statement);
  if (ts.isFunctionDeclaration(statement) && statement.name) return transformFunctionDeclaration(statement);
  return printNode(transformWithScopes(statement, []));
}

const functionStatements = [];
const orderedBodyStatements = [];

for (const statement of sourceFile.statements) {
  if (ts.isImportDeclaration(statement) || ts.isTypeAliasDeclaration(statement)) continue;
  if (ts.isFunctionDeclaration(statement) && statement.name) {
    functionStatements.push(transformFunctionDeclaration(statement));
  } else {
    orderedBodyStatements.push(transformStatement(statement));
  }
}

const commonPrefix = `// @ts-nocheck\nimport type { MessengerControllerContext } from './messengerControllerContext';\n${importsText}\n\n${typeAliasText}\n`;

function makeInstaller(name, statements) {
  const body = statements
    .join('\n\n')
    .split('\n')
    .map((line) => (line ? `  ${line}` : ''))
    .join('\n');
  return `${commonPrefix}\nexport function ${name}(ctx: MessengerControllerContext): void {\n${body}\n}\n`;
}

function withModuleDescription(content, description) {
  return description ? content.replace('// @ts-nocheck\n', `// @ts-nocheck\n// ${description}\n`) : content;
}

const chunks = [];
let currentChunk = [];
let currentLines = 0;

for (const statementText of orderedBodyStatements) {
  const nextLines = lineCount(statementText) + 1;
  if (currentChunk.length && currentLines + nextLines > targetBodyLines) {
    chunks.push(currentChunk);
    currentChunk = [];
    currentLines = 0;
  }
  currentChunk.push(statementText);
  currentLines += nextLines;
}
if (currentChunk.length) {
  chunks.push(currentChunk);
}

const partFiles = chunks.map((chunk, index) => {
  const module = controllerModules[index] || {
    file: `messengerControllerOverflow${index + 1}.ts`,
    installer: `installMessengerControllerOverflow${index + 1}`
  };
  return {
    name: module.file,
    installer: module.installer,
    description: module.description || '',
    statements: chunk
  };
});

const secondaryControllerSplits = {
  'messengerControllerCoreState.ts': [
    {
      file: 'messengerControllerStateRefs.ts',
      installer: 'installMessengerControllerStateRefs',
      anchor: 'ctx.route =',
      description: 'Store wiring, mutable refs, runtime handles, cache state, and performance tracing.'
    },
    {
      file: 'messengerControllerShellLayoutState.ts',
      installer: 'installMessengerControllerShellLayoutState',
      anchor: 'ctx.sectionOptions =',
      description: 'Messenger shell navigation, desktop mode, responsive panes, and host layout state.'
    },
    {
      file: 'messengerControllerAgentIdentityState.ts',
      installer: 'installMessengerControllerAgentIdentityState',
      anchor: 'ctx.DEFAULT_BEEROOM_GROUP_ID =',
      description: 'Agent identity, active session model display, approval mode, and default profile state.'
    }
  ],
  'messengerControllerNavigationLists.ts': [
    {
      file: 'messengerControllerRuntimeToolLists.ts',
      installer: 'installMessengerControllerRuntimeToolLists',
      anchor: 'ctx.pendingApprovalAgentIdSet =',
      description: 'Runtime busy state, prompt ability summaries, right-dock skills, file containers, and settings targets.'
    },
    {
      file: 'messengerControllerContactLists.ts',
      installer: 'installMessengerControllerContactLists',
      anchor: 'ctx.contactUnitLabelMap =',
      description: 'Contact unit tree, contact filtering, group filtering, and virtual contact list projections.'
    },
    {
      file: 'messengerControllerHiveMixedLists.ts',
      installer: 'installMessengerControllerHiveMixedLists',
      anchor: 'ctx.filteredPlazaItems =',
      description: 'Plaza, beeroom, agent ordering, mixed conversations, and drag-order projections.'
    },
    {
      file: 'messengerControllerPanelSummaries.ts',
      installer: 'installMessengerControllerPanelSummaries',
      anchor: 'ctx.activeConversationTitle =',
      description: 'Conversation titles, page waiting state, chat footer state, and dismissed conversation persistence.'
    }
  ],
  'messengerControllerConversationRuntime.ts': [
    {
      file: 'messengerControllerAgentUnreadRuntime.ts',
      installer: 'installMessengerControllerAgentUnreadRuntime',
      anchor: 'ctx.ensureAgentUnreadState =',
      description: 'Agent main-session unread state, preferred session prefetching, and unread persistence.'
    },
    {
      file: 'messengerControllerWorldComposerRuntime.ts',
      installer: 'installMessengerControllerWorldComposerRuntime',
      anchor: 'ctx.loadStoredStringArray =',
      description: 'World drafts, history filters, composer resizing, emoji picker, container picker, and quick panel behavior.'
    },
    {
      file: 'messengerControllerRightDockSessionRuntime.ts',
      installer: 'installMessengerControllerRightDockSessionRuntime',
      anchor: 'ctx.fileContainerLifecycleText =',
      description: 'File lifecycle text, right dock visibility, right-panel session history, and timeline preview caching.'
    },
    {
      file: 'messengerControllerAgentRuntimeSignals.ts',
      installer: 'installMessengerControllerAgentRuntimeSignals',
      anchor: 'ctx.hasCronTask =',
      description: 'Cron badges, agent runtime state normalization, hot-state detection, and completion notifications.'
    },
    {
      file: 'messengerControllerRenderableMessages.ts',
      installer: 'installMessengerControllerRenderableMessages',
      anchor: 'ctx.hasMessageContent =',
      description: 'User attachments, agent/world renderable message lists, virtualization helpers, and plan state.'
    }
  ],
  'messengerControllerMessageResources.ts': [
    {
      file: 'messengerControllerMessagePanelsPresentation.ts',
      installer: 'installMessengerControllerMessagePanelsPresentation',
      anchor: 'ctx.dismissActiveAgentPlan =',
      description: 'Plan and inquiry panels, avatar labels, timestamps, presence labels, and admin checks.'
    },
    {
      file: 'messengerControllerWorkspaceResourceHydration.ts',
      installer: 'installMessengerControllerWorkspaceResourceHydration',
      anchor: 'ctx.resolveDesktopWorkspaceRoot =',
      description: 'Workspace path resolution, resource fetching, markdown resource cards, image preview, and resource downloads.'
    },
    {
      file: 'messengerControllerMessageMarkdownVoice.ts',
      installer: 'installMessengerControllerMessageMarkdownVoice',
      anchor: 'ctx.trimMarkdownCache =',
      description: 'Markdown rendering, world voice playback, assistant resume, copy actions, and world message identity.'
    },
    {
      file: 'messengerControllerMessageRoutingPreferences.ts',
      installer: 'installMessengerControllerMessageRoutingPreferences',
      anchor: 'ctx.resolveAgentMessageKey =',
      description: 'Message keys, route sync, section switching, middle-pane delegates, appearance, ordering, and beeroom caches.'
    }
  ],
  'messengerControllerWorkspaceActions.ts': [
    {
      file: 'messengerControllerWorkspaceOrderUiActions.ts',
      installer: 'installMessengerControllerWorkspaceOrderUiActions',
      anchor: 'ctx.prioritizeImportedBeeroomAgents =',
      description: 'Messenger order persistence, current-user appearance, launch behavior, middle-pane overlay, and quick agent creation.'
    },
    {
      file: 'messengerControllerBeeroomAgentMutations.ts',
      installer: 'installMessengerControllerBeeroomAgentMutations',
      anchor: 'ctx.refreshActiveBeeroom =',
      description: 'Beeroom refreshes, agent mutation refreshes, worker-card import, and batch agent actions.'
    },
    {
      file: 'messengerControllerConversationOpenActions.ts',
      installer: 'installMessengerControllerConversationOpenActions',
      anchor: 'ctx.handleSearchCreateAction =',
      description: 'Search-create routing, middle-pane selections, world conversation openers, agent sessions, and prompt previews.'
    },
    {
      file: 'messengerControllerTimelineFileActions.ts',
      installer: 'installMessengerControllerTimelineFileActions',
      anchor: 'ctx.restoreTimelineSession =',
      description: 'Timeline session operations and file container context-menu actions.'
    }
  ],
  'messengerControllerMessagingSettings.ts': [
    {
      file: 'messengerControllerFileToolSettings.ts',
      installer: 'installMessengerControllerFileToolSettings',
      anchor: 'ctx.handleFileContainerMenuCopyId =',
      description: 'File container selection, tool catalog loading, organization units, and active agent refresh helpers.'
    },
    {
      file: 'messengerControllerAgentMessageCommands.ts',
      installer: 'installMessengerControllerAgentMessageCommands',
      anchor: 'ctx.handleAgentSettingsSaved =',
      description: 'Agent settings save/delete reactions, section selection fallback, local commands, agent send, and stop actions.'
    },
    {
      file: 'messengerControllerWorldMessagingActions.ts',
      installer: 'installMessengerControllerWorldMessagingActions',
      anchor: 'ctx.normalizeUploadPath =',
      description: 'World upload paths, helper apps, screenshots, desktop model metadata, voice recording, and world sending.'
    },
    {
      file: 'messengerControllerClientPreferenceActions.ts',
      installer: 'installMessengerControllerClientPreferenceActions',
      anchor: 'ctx.toggleLanguage =',
      description: 'Language switching, desktop update checks, send-key/profile preferences, approvals, theme, and debug tools.'
    }
  ],
  'messengerControllerLifecycle.ts': [
    {
      file: 'messengerControllerLifecycleRuntimeMeta.ts',
      installer: 'installMessengerControllerLifecycleRuntimeMeta',
      anchor: 'ctx.loadRunningAgents =',
      description: 'Runtime metadata refreshers for agents, cron jobs, channel bindings, realtime contacts, and full refresh.'
    },
    {
      file: 'messengerControllerLifecycleMessageViewport.ts',
      installer: 'installMessengerControllerLifecycleMessageViewport',
      anchor: 'ctx.syncMessageVirtualMetrics =',
      description: 'Message viewport runtime wrappers, virtual measurement, scroll controls, and latest assistant layout refresh.'
    },
    {
      file: 'messengerControllerLifecycleRouteBootstrap.ts',
      installer: 'installMessengerControllerLifecycleRouteBootstrap',
      anchor: 'ctx.restoreConversationFromRoute =',
      description: 'Route restoration, bootstrap loading, keyword synchronization, middle-pane overlay syncing, and route-driven view state.'
    },
    {
      file: 'messengerControllerLifecycleReactiveEffects.ts',
      installer: 'installMessengerControllerLifecycleReactiveEffects',
      anchor: 'watch(() => ctx.currentUserId.value',
      description: 'Cross-domain watchers, mounted listeners, realtime pulse wiring, and unmount cleanup.'
    }
  ]
};

function findStatementIndex(statements, anchor) {
  return statements.findIndex((statement) => statement.trimStart().startsWith(anchor));
}

function splitModuleStatements(module) {
  const split = secondaryControllerSplits[module.name];
  if (!split) {
    return {
      aggregator: null,
      files: [
        {
          name: module.name,
          installer: module.installer,
          content: withModuleDescription(makeInstaller(module.installer, module.statements), module.description)
        }
      ]
    };
  }

  const anchors = split.map((part) => {
    const index = findStatementIndex(module.statements, part.anchor);
    if (index < 0) {
      throw new Error(`Could not find anchor "${part.anchor}" in ${module.name}.`);
    }
    return { ...part, index };
  });
  anchors.sort((left, right) => left.index - right.index);
  if (anchors[0].index !== 0) {
    throw new Error(`First secondary split for ${module.name} must start at the first statement.`);
  }

  const files = anchors.map((part, index) => {
    const next = anchors[index + 1];
    const statements = module.statements.slice(part.index, next ? next.index : module.statements.length);
    return {
      name: part.file,
      installer: part.installer,
      content: withModuleDescription(makeInstaller(part.installer, statements), part.description)
    };
  });

  const aggregatorContent = [
    `import type { MessengerControllerContext } from './messengerControllerContext';`,
    ...files.map((file) => `import { ${file.installer} } from './${file.name.replace(/\.ts$/, '')}';`),
    '',
    `const installers = [`,
    ...files.map((file) => `  ${file.installer},`),
    `];`,
    '',
    `export function ${module.installer}(ctx: MessengerControllerContext): void {`,
    `  for (const install of installers) {`,
    `    install(ctx);`,
    `  }`,
    `}`,
    ''
  ].join('\n');

  return {
    aggregator: {
      name: module.name,
      installer: module.installer,
      content: aggregatorContent
    },
    files
  };
}

const generatedControllerModules = partFiles.flatMap((module) => {
  const split = splitModuleStatements(module);
  return split.aggregator ? [split.aggregator, ...split.files] : split.files;
});

const allReturnNames = [...new Set([...importValueNames, ...topNames])].sort((left, right) => left.localeCompare(right));
const rootImports = [
  importsText,
  `import type { MessengerControllerContext } from './controller/messengerControllerContext';`,
  `import { installMessengerController } from './controller/installMessengerController';`
].join('\n');

const rootFile = `${rootImports}\n\nexport function useMessengerViewController(): Record<string, any> {\n  const ctx: MessengerControllerContext = {};\n  installMessengerController(ctx);\n  return {\n${importValueNames
    .sort((left, right) => left.localeCompare(right))
    .map((name) => `    ${name},`)
    .join('\n')}\n    ...ctx\n  };\n}\n`;

const scriptLines = [
  '<script setup lang="ts">',
  "import { useMessengerViewController } from '@/views/messenger/useMessengerViewController';",
  '',
  'const controller = useMessengerViewController();',
  ...allReturnNames.map((name) => `const ${name} = controller.${name};`),
  '</script>'
];

const vueFile = `${templateBlock}\n\n${scriptLines.join('\n')}\n`;

fs.mkdirSync(controllerDir, { recursive: true });
for (const file of fs.readdirSync(controllerDir)) {
  if (/^messengerControllerPart\d+\.ts$/.test(file) || file === 'messengerControllerFunctions.ts') {
    fs.rmSync(path.join(controllerDir, file));
  }
}
fs.writeFileSync(sourcePath, vueFile, 'utf8');
fs.writeFileSync(path.join(messengerDir, 'useMessengerViewController.ts'), rootFile, 'utf8');
fs.writeFileSync(path.join(controllerDir, 'messengerControllerContext.ts'), 'export type MessengerControllerContext = any;\n', 'utf8');
fs.writeFileSync(
  path.join(controllerDir, 'messengerControllerSharedHelpers.ts'),
  makeInstaller('installMessengerControllerSharedHelpers', functionStatements),
  'utf8'
);
for (const file of generatedControllerModules) {
  fs.writeFileSync(path.join(controllerDir, file.name), file.content, 'utf8');
}
fs.writeFileSync(
  path.join(controllerDir, 'installMessengerController.ts'),
  `import type { MessengerControllerContext } from './messengerControllerContext';\nimport { installMessengerControllerSharedHelpers } from './messengerControllerSharedHelpers';\n${partFiles
    .map((file) => `import { ${file.installer} } from './${file.name.replace(/\.ts$/, '')}';`)
    .join('\n')}\n\nconst installers = [\n  installMessengerControllerSharedHelpers,\n${partFiles
    .map((file) => `  ${file.installer}`)
    .join(',\n')}\n];\n\nexport function installMessengerController(ctx: MessengerControllerContext): void {\n  for (const install of installers) {\n    install(ctx);\n  }\n}\n`,
  'utf8'
);
const installControllerContent = fs.readFileSync(path.join(controllerDir, 'installMessengerController.ts'), 'utf8');

const generatedFiles = [
  ['frontend/src/views/MessengerView.vue', vueFile],
  ['frontend/src/views/messenger/useMessengerViewController.ts', rootFile],
  ['frontend/src/views/messenger/controller/messengerControllerContext.ts', 'export type MessengerControllerContext = any;\n'],
  ['frontend/src/views/messenger/controller/messengerControllerSharedHelpers.ts', makeInstaller('installMessengerControllerSharedHelpers', functionStatements)],
  [
    'frontend/src/views/messenger/controller/installMessengerController.ts',
    installControllerContent
  ],
  ...generatedControllerModules.map((file) => [`frontend/src/views/messenger/controller/${file.name}`, file.content])
];

console.log(
  JSON.stringify(
    {
      topNames: topNames.length,
      importValueNames: importValueNames.length,
      parts: partFiles.length,
      files: generatedFiles.map(([name, content]) => ({ name, lines: lineCount(content) }))
    },
    null,
    2
  )
);
