import type { WritConfig } from "../../src/types/config";

// writ_core::config::WritConfig::default(), serialised, with the notes folder
// open in the sidebar and the light theme pinned.
export const DEMO_CONFIG: WritConfig = {
  "hotkey": {
    "toggle": "CmdOrCtrl+Shift+Space"
  },
  "sidebar": {
    "toggle": "CmdOrCtrl+\\",
    "default_visible": false,
    "position": "left",
    "open": true,
    "width": 240,
    "collapsed": [],
    "hidden": [
      "recent"
    ]
  },
  "panel": {
    "open": false,
    "width": 240
  },
  "chat_panel": {
    "open": false,
    "width": 380
  },
  "first_run": {
    "hint_dismissed": true
  },
  "editor": {
    "font_family": "monospace",
    "font_size": 16,
    "word_wrap": true,
    "tab_size": 2,
    "autosave_debounce_ms": 1000,
    "markdown_typography": true,
    "markdown_editing": true,
    "status_bar": true,
    "status_bar_counts": false
  },
  "files": {
    "default_extension": "txt"
  },
  "window": {
    "width": 1280,
    "height": 800,
    "maximized": false
  },
  "keybindings": {},
  "history": {
    "max_entries": 500
  },
  "storage": {
    "path": "~/.writ"
  },
  "theme": {
    "preset": "writ-light",
    "overrides": {}
  },
  "appearance": {
    "polarity": "light",
    "accent": "pine",
    "prose_face": "system",
    "interface_text_size": null
  },
  "commands": {
    "usage": {}
  },
  "preview": {
    "default_layout_html": "split",
    "default_layout_markdown": "inline",
    "live_render_threshold_mb": 1,
    "render_confirm_threshold_mb": 5,
    "render_refuse_threshold_mb": 50,
    "debounce_ms": 200,
    "run_scripts": true
  },
  "workspace": {
    "root": "/Users/you/Notes"
  },
  "inbox": {
    "path": null,
    "focus": true
  },
  "updater": {
    "auto_check": false
  },
  "ai": {
    "provider": "ollama",
    "base_url": "",
    "model": "",
    "consented_hosts": [],
    "rewrite": {
      "enabled": false
    },
    "chat": {
      "enabled": false,
      "model": "",
      "model_provider": ""
    }
  },
  "mcp": {
    "enabled": false,
    "approved_clients": []
  },
  "spelling": {
    "enabled": false,
    "dialect": "american",
    "ignored_words": []
  },
  "apps": {
    "connections": false,
    "graph": false,
    "tags": false
  }
};
