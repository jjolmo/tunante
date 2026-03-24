import { invoke } from '@tauri-apps/api/core';
import { libraryStore } from '$lib/stores/library.svelte';
import { settingsStore } from '$lib/stores/settings.svelte';
import type { Track } from '$lib/types';

export interface FolderNode {
	name: string;
	fullPath: string;
	trackCount: number;
	children: FolderNode[];
}

class FilesStore {
	viewMode = $state<'tree' | 'breadcrumb'>('tree');
	activeFolder = $state<string | null>(null);
	expandedFolders = $state<Set<string>>(new Set());
	currentPath = $state<string | null>(null);
	folderSearch = $state('');

	// --- Derived: folder tree built from track paths ---
	get folderTree(): FolderNode[] {
		const tracks = libraryStore.tracks;
		if (tracks.length === 0) return [];

		// Step 1: Count tracks per directory
		const dirCounts = new Map<string, number>();
		for (const track of tracks) {
			const lastSlash = track.path.lastIndexOf('/');
			if (lastSlash === -1) continue;
			const dir = track.path.substring(0, lastSlash);
			dirCounts.set(dir, (dirCounts.get(dir) || 0) + 1);
		}

		// Step 2: Find roots from monitored folders
		const roots = settingsStore.monitoredFolders.map((f) => f.path);
		if (roots.length === 0) {
			// Fallback: find common prefix of all directories
			const dirs = [...dirCounts.keys()].sort();
			if (dirs.length > 0) {
				roots.push(findCommonPrefix(dirs));
			}
		}

		// Step 3: Build tree for each root
		const result: FolderNode[] = [];
		for (const root of roots) {
			const tree = buildTree(root, dirCounts);
			if (tree) result.push(tree);
		}

		return result;
	}

	// --- Derived: flat list of folders matching search query ---
	get folderSearchResults(): FolderNode[] {
		const q = this.folderSearch.trim().toLowerCase();
		if (!q) return [];
		const results: FolderNode[] = [];
		const collectMatches = (nodes: FolderNode[]) => {
			for (const node of nodes) {
				if (node.name.toLowerCase().includes(q)) {
					results.push(node);
				}
				collectMatches(node.children);
			}
		};
		collectMatches(this.folderTree);
		return results.slice(0, 50); // Limit to 50 results
	}

	// --- Derived: tracks for the active folder ---
	get folderTracks(): Track[] {
		if (!this.activeFolder) return [];
		const prefix = this.activeFolder + '/';
		return libraryStore.filteredTracks.filter(
			(t) => t.path.startsWith(prefix) || t.path.substring(0, t.path.lastIndexOf('/')) === this.activeFolder
		);
	}

	// --- Derived: breadcrumb segments ---
	get breadcrumbs(): { name: string; path: string }[] {
		if (!this.currentPath) return [];

		// Find the monitored folder root that contains currentPath
		const root = settingsStore.monitoredFolders.find((f) =>
			this.currentPath!.startsWith(f.path)
		);
		const rootPath = root ? root.path : '';
		const remaining = this.currentPath.substring(rootPath.length);
		const segments = remaining.split('/').filter(Boolean);

		const crumbs: { name: string; path: string }[] = [];
		// Add root
		if (rootPath) {
			const rootName = rootPath.substring(rootPath.lastIndexOf('/') + 1);
			crumbs.push({ name: rootName, path: rootPath });
		}
		// Add each segment
		let accumulated = rootPath;
		for (const seg of segments) {
			accumulated += '/' + seg;
			crumbs.push({ name: seg, path: accumulated });
		}
		return crumbs;
	}

	// --- Derived: children of currentPath (for breadcrumb mode) ---
	get currentFolderChildren(): FolderNode[] {
		if (!this.currentPath) return this.folderTree;
		return findNodeChildren(this.folderTree, this.currentPath);
	}

	// --- Actions ---
	selectFolder(path: string | null) {
		this.activeFolder = path;
		if (path) {
			invoke('set_setting', { key: 'session_view', value: 'files' }).catch(() => {});
			invoke('set_setting', { key: 'session_view_id', value: path }).catch(() => {});
		}
	}

	toggleExpanded(path: string) {
		const next = new Set(this.expandedFolders);
		if (next.has(path)) {
			next.delete(path);
		} else {
			next.add(path);
		}
		this.expandedFolders = next;
	}

	navigateTo(path: string | null) {
		this.currentPath = path;
		this.selectFolder(path);
	}

	navigateUp() {
		if (!this.currentPath) return;
		const parent = this.currentPath.substring(0, this.currentPath.lastIndexOf('/'));
		const isUnderRoot = settingsStore.monitoredFolders.some((f) =>
			parent.startsWith(f.path) && parent.length >= f.path.length
		);
		this.navigateTo(isUnderRoot ? parent : null);
	}

	setViewMode(mode: 'tree' | 'breadcrumb') {
		this.viewMode = mode;
		invoke('set_setting', { key: 'files_view_mode', value: mode }).catch(() => {});
	}

	restoreFromCache(getSetting: (key: string) => string | null) {
		const mode = getSetting('files_view_mode');
		if (mode === 'tree' || mode === 'breadcrumb') this.viewMode = mode;

		const expanded = getSetting('files_expanded_folders');
		if (expanded) {
			try {
				this.expandedFolders = new Set(JSON.parse(expanded));
			} catch {}
		}
	}

	saveExpandedFolders() {
		invoke('set_setting', {
			key: 'files_expanded_folders',
			value: JSON.stringify([...this.expandedFolders]),
		}).catch(() => {});
	}
}

export const filesStore = new FilesStore();

// --- Helper functions ---

function findCommonPrefix(dirs: string[]): string {
	if (dirs.length === 0) return '';
	if (dirs.length === 1) return dirs[0];
	let prefix = dirs[0];
	for (let i = 1; i < dirs.length; i++) {
		while (!dirs[i].startsWith(prefix)) {
			const lastSlash = prefix.lastIndexOf('/');
			if (lastSlash === -1) return '';
			prefix = prefix.substring(0, lastSlash);
		}
	}
	return prefix;
}

function buildTree(rootPath: string, dirCounts: Map<string, number>): FolderNode | null {
	// Collect all dirs under this root
	const childDirs: string[] = [];
	for (const [dir] of dirCounts) {
		if (dir === rootPath || dir.startsWith(rootPath + '/')) {
			childDirs.push(dir);
		}
	}
	if (childDirs.length === 0) return null;

	// Build a trie-like structure
	const rootName = rootPath.substring(rootPath.lastIndexOf('/') + 1) || rootPath;
	const root: FolderNode = {
		name: rootName,
		fullPath: rootPath,
		trackCount: 0,
		children: [],
	};

	// Map of fullPath → FolderNode
	const nodeMap = new Map<string, FolderNode>();
	nodeMap.set(rootPath, root);

	// Sort directories so parents come before children
	childDirs.sort();

	for (const dir of childDirs) {
		if (dir === rootPath) {
			root.trackCount += dirCounts.get(dir) || 0;
			continue;
		}

		// Find or create all intermediate nodes
		const relativeParts = dir.substring(rootPath.length + 1).split('/');
		let currentPath = rootPath;
		let parentNode = root;

		for (const part of relativeParts) {
			currentPath += '/' + part;
			let node = nodeMap.get(currentPath);
			if (!node) {
				node = {
					name: part,
					fullPath: currentPath,
					trackCount: 0,
					children: [],
				};
				nodeMap.set(currentPath, node);
				parentNode.children.push(node);
			}
			parentNode = node;
		}

		parentNode.trackCount += dirCounts.get(dir) || 0;
	}

	// Propagate counts upward (post-order traversal)
	propagateCounts(root);

	// Collapse single-child chains (compact folders like VS Code)
	compactTree(root);

	return root;
}

function propagateCounts(node: FolderNode): number {
	let total = node.trackCount;
	for (const child of node.children) {
		total += propagateCounts(child);
	}
	node.trackCount = total;
	return total;
}

/** Collapse single-child intermediate nodes: A > B > C → A/B/C */
function compactTree(node: FolderNode) {
	for (const child of node.children) {
		compactTree(child);
	}
	// If a child has exactly one child and no direct tracks of its own
	// (all its tracks come from the grandchild), merge them
	for (let i = 0; i < node.children.length; i++) {
		const child = node.children[i];
		while (
			child.children.length === 1 &&
			child.trackCount === child.children[0].trackCount
		) {
			const grandchild = child.children[0];
			child.name = child.name + '/' + grandchild.name;
			child.fullPath = grandchild.fullPath;
			child.children = grandchild.children;
			// trackCount stays the same (already propagated)
		}
	}
}

function findNodeChildren(trees: FolderNode[], path: string): FolderNode[] {
	for (const node of trees) {
		if (node.fullPath === path) return node.children;
		if (path.startsWith(node.fullPath + '/')) {
			const found = findNodeChildren(node.children, path);
			if (found.length > 0) return found;
		}
	}
	return [];
}
