# Workspace Inspection

A terminal workspace inspector for navigating a bounded directory tree, viewing files, and understanding local Git changes without modifying files or repository state.

## Language

**Workspace Inspector**:
The read-only terminal application that combines file navigation, file viewing, and Git change inspection within one workspace.
_Avoid_: File manager, editor

**Workspace Root**:
The topmost directory visible during a session, selected from a launch argument or the process's current directory. Navigation cannot move above it.
_Avoid_: Project root, repository root, current directory

**Workspace**:
The directory tree rooted at the Workspace Root and inspected during one application session. It may exist outside a Git repository.
_Avoid_: Project, repository

**Files View**:
The top-level screen for navigating the Workspace as an expandable File Tree.
_Avoid_: Explorer view, file tree view

**File Preview**:
A full-screen, read-only rendering of a file selected from the Files View. Returning preserves the File Tree's prior state.
_Avoid_: Preview pane, viewer pane

**Changes View**:
The top-level screen listing Git changes as Conflicts, Staged Changes, Unstaged Changes, and Untracked Files.
_Avoid_: Source control view, Git diff view

**Change Entry**:
A path paired with one Git comparison state. The same path may have separate staged and unstaged Change Entries.
_Avoid_: Changed file

**Diff View**:
A full-screen, read-only rendering of the Git change represented by one Change Entry. Returning preserves the Changes View's prior state.
_Avoid_: Diff pane

**argos-explorer**:
The Workspace Inspector's product name and command-line executable.
_Avoid_: Explorer, explorer, Argos Explorer

**Quick Open**:
The global fuzzy file finder used to locate a file anywhere in the Workspace and open its File Preview.
_Avoid_: Fuzzy finder, file search

**Icon Mode**:
The configured file-and-folder icon vocabulary: Nerd Font glyphs, controlled emoji, or no icons.
_Avoid_: Automatic font detection
