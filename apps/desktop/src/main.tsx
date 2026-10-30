import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { prepareDocument } from './lib/appearance';
import './styles/index.css';

// /boot.js put theme, contrast and motion on <html> already; the font and the
// language follow before the first render.
prepareDocument();

const root = document.getElementById('root');
if (!root) throw new Error('#root missing from index.html');

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
