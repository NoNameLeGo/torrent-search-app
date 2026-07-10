'use strict';

const axios = require('axios');

const USER_AGENTS = [
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36',
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0',
];

function pickUA() {
  return USER_AGENTS[Math.floor(Math.random() * USER_AGENTS.length)];
}

// Shared axios instance with sane timeouts and a browser-like UA.
const http = axios.create({
  timeout: 10000,
  maxRedirects: 5,
  headers: {
    'Accept-Language': 'en-US,en;q=0.9',
    'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
  },
});

// Perform a GET returning { html, status, error }. Never throws.
async function getText(url, opts = {}) {
  try {
    const res = await http.get(url, {
      responseType: 'text',
      headers: { 'User-Agent': pickUA(), ...(opts.headers || {}) },
      ...opts,
    });
    return { html: res.data, status: res.status, error: null };
  } catch (e) {
    return { html: null, status: null, error: e.code || e.message || 'request_failed' };
  }
}

// Perform a GET returning parsed JSON. Never throws.
async function getJSON(url, opts = {}) {
  try {
    const res = await http.get(url, {
      responseType: 'json',
      headers: { 'User-Agent': pickUA(), ...(opts.headers || {}) },
      ...opts,
    });
    return { data: res.data, status: res.status, error: null };
  } catch (e) {
    return { data: null, status: null, error: e.code || e.message || 'request_failed' };
  }
}

// Perform a POST with a JSON body returning parsed JSON. Never throws.
async function postJSON(url, payload, opts = {}) {
  try {
    const res = await http.post(url, payload, {
      responseType: 'json',
      headers: { 'User-Agent': pickUA(), ...(opts.headers || {}) },
      ...opts,
    });
    return { data: res.data, status: res.status, error: null };
  } catch (e) {
    return { data: null, status: null, error: e.code || e.message || 'request_failed' };
  }
}

module.exports = { http, getText, getJSON, postJSON, pickUA };
