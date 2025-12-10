import { handle_event } from './pkg/svelters_output.js';

function applyPatch(patch, nodeMap) {
    if (patch.operation.SetContent) {
        const node = nodeMap[patch.target_id];
        node.textContent = patch.operation.SetContent.value;
    } else if (patch.operation.SetAttribute) {
        const node = nodeMap[patch.target_id];
        const { name, value } = patch.operation.SetAttribute;
        node.setAttribute(name, value);
    }
}

export function applyPatches(patches, nodeMap) {
    for (const patch of patches) {
        applyPatch(patch, nodeMap);
    }
}

export async function handle_js_event(e, nodeMap, target_id) {
    const patches = handle_event(e, target_id);
    applyPatches(patches, nodeMap);
}