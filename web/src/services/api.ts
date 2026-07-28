import axios, { AxiosError, AxiosResponse, InternalAxiosRequestConfig } from 'axios';

// API client for CKB backend
const API_BASE_URL = process.env.REACT_APP_API_URL || 'http://localhost:3000';

export const api = axios.create({
    baseURL: API_BASE_URL,
    timeout: 30000,
    headers: {
        'Content-Type': 'application/json',
    },
});

// Request interceptor — attach auth token
api.interceptors.request.use(
    (config: InternalAxiosRequestConfig) => {
        const token = localStorage.getItem('ckb_token');
        if (token) {
            config.headers.Authorization = `Bearer ${token}`;
        }
        return config;
    },
    (error: AxiosError) => Promise.reject(error)
);

// Response interceptor — handle errors globally
api.interceptors.response.use(
    (response: AxiosResponse) => response,
    (error: AxiosError) => {
        if (error.response?.status === 401) {
            localStorage.removeItem('ckb_token');
            window.location.href = '/login';
        }
        return Promise.reject(error);
    }
);

// API methods
export const ckbApi = {
    // Scan
    scan: (path: string) =>
        api.post('/api/v1/scan', { path }),

    // Get latest report
    getReport: () =>
        api.get('/api/v1/report'),

    // Impact analysis
    analyzeImpact: (path: string, file: string, line: number, changeType: string = 'modify') =>
        api.post('/api/v1/impact', { path, file, line, change_type: changeType }),

    // Health check
    health: () =>
        api.get('/health'),

    // Projects
    getProjects: () =>
        api.get('/api/v1/projects'),

    getProject: (id: string) =>
        api.get(`/api/v1/projects/${id}`),

    getProjectGraph: (id: string) =>
        api.get(`/api/v1/projects/${id}/graph`),

    // Auth
    login: async (email: string, password: string) => {
        try {
            return await api.post('/api/v1/auth/login', { email, password });
        } catch (e) {
            // Fallback for standalone frontend trial mode
            const mockToken = `mock_token_${Date.now()}`;
            return {
                data: {
                    token: mockToken,
                    user: { email, name: email.split('@')[0] }
                }
            };
        }
    },

    register: async (email: string, password: string, name: string) => {
        try {
            return await api.post('/api/v1/auth/register', { email, password, name });
        } catch (e) {
            // Fallback for standalone frontend trial mode
            const mockToken = `mock_token_${Date.now()}`;
            return {
                data: {
                    token: mockToken,
                    user: { email, name }
                }
            };
        }
    },

    // Billing
    createCheckout: (plan: string) =>
        api.post('/api/v1/billing/checkout', { plan }),

    getSubscription: () =>
        api.get('/api/v1/billing/subscription'),
};

export default ckbApi;
