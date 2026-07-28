import axios, { AxiosError, AxiosResponse, InternalAxiosRequestConfig } from 'axios';

const ENGINE_BASE_URL = process.env.REACT_APP_API_URL || 'http://localhost:3000';
const BACKEND_BASE_URL = process.env.REACT_APP_BACKEND_URL || 'http://localhost:4000';

// API client for Rust analysis engine
export const api = axios.create({
    baseURL: ENGINE_BASE_URL,
    timeout: 30000,
    headers: {
        'Content-Type': 'application/json',
    },
});

// API client for Node authentication & billing backend
export const backendApi = axios.create({
    baseURL: BACKEND_BASE_URL,
    timeout: 30000,
    headers: {
        'Content-Type': 'application/json',
    },
});

// Apply interceptors to an Axios instance
const applyInterceptors = (instance: any) => {
    // Request interceptor — attach auth token
    instance.interceptors.request.use(
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
    instance.interceptors.response.use(
        (response: AxiosResponse) => response,
        (error: AxiosError) => {
            if (error.response?.status === 401) {
                localStorage.removeItem('ckb_token');
                window.location.href = '/login';
            }
            return Promise.reject(error);
        }
    );
};

applyInterceptors(api);
applyInterceptors(backendApi);

// API methods
export const ckbApi = {
    // Scan (Rust Engine)
    scan: (path: string) =>
        api.post('/api/v1/scan', { path }),

    // Get latest report (Rust Engine)
    getReport: () =>
        api.get('/api/v1/report'),

    // Impact analysis (Rust Engine)
    analyzeImpact: (path: string, file: string, line: number, changeType: string = 'modify') =>
        api.post('/api/v1/impact', { path, file, line, change_type: changeType }),

    // Health check (Rust Engine)
    health: () =>
        api.get('/health'),

    // Projects (Node Backend)
    getProjects: () =>
        backendApi.get('/api/v1/projects'),

    getProject: (id: string) =>
        backendApi.get(`/api/v1/projects/${id}`),

    getProjectGraph: (id: string) =>
        backendApi.get(`/api/v1/projects/${id}/graph`),

    // Auth (Node Backend)
    login: async (email: string, password: string) => {
        try {
            return await backendApi.post('/api/v1/auth/login', { email, password });
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
            return await backendApi.post('/api/v1/auth/register', { email, password, name });
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

    // Billing (Node Backend)
    createCheckout: (plan: string) =>
        backendApi.post('/api/v1/billing/checkout', { plan }),

    getSubscription: () =>
        backendApi.get('/api/v1/billing/subscription'),
};

export default ckbApi;
