import { mount } from 'svelte';
import App from './App.svelte';

// Stealth-probe sentinel page (tools/stealth-probe, debug builds): a solid colour, no app.
if (location.hash === '#probe') {
  document.documentElement.style.background = document.body.style.background = '#00ff00';
} else {
  mount(App, { target: document.getElementById('app')! });
}
