// Tracking events for product analytics

export const AnalyticsEvents = {
    // User events
    userSignedUp: {
        name: 'user_signed_up',
        properties: ['plan', 'source', 'referrer']
    },
    userSubscribed: {
        name: 'user_subscribed',
        properties: ['plan', 'interval', 'amount', 'coupon']
    },
    userCanceled: {
        name: 'user_canceled',
        properties: ['plan', 'reason', 'feedback']
    },

    // Product events
    projectScanned: {
        name: 'project_scanned',
        properties: ['files', 'language', 'duration']
    },
    violationsDetected: {
        name: 'violations_detected',
        properties: ['count', 'severity', 'types']
    },
    impactAnalyzed: {
        name: 'impact_analyzed',
        properties: ['file', 'line', 'risk_score']
    },

    // Integration events
    mcpServerStarted: {
        name: 'mcp_server_started',
        properties: ['version', 'duration']
    },
    vscodeInstalled: {
        name: 'vscode_installed',
        properties: ['version']
    },
    cliUsed: {
        name: 'cli_used',
        properties: ['command', 'args']
    },

    // Engagement metrics
    weeklyActive: {
        name: 'weekly_active',
        properties: ['userId', 'date']
    },
    featureUsed: {
        name: 'feature_used',
        properties: ['feature', 'count']
    },
    timeInProduct: {
        name: 'time_in_product',
        properties: ['session_duration', 'pages_viewed']
    },

    // Conversion funnel
    viewedPricing: {
        name: 'viewed_pricing',
        properties: ['source']
    },
    startedCheckout: {
        name: 'started_checkout',
        properties: ['plan', 'interval']
    },
    completedCheckout: {
        name: 'completed_checkout',
        properties: ['plan', 'interval', 'amount']
    },

    // Retention metrics
    day1Retention: {
        name: 'day1_retention',
        properties: ['returned']
    },
    day7Retention: {
        name: 'day7_retention',
        properties: ['returned']
    },
    day30Retention: {
        name: 'day30_retention',
        properties: ['returned']
    }
};

// Tracking implementation
export class Analytics {
    private apiKey: string;
    private userId?: string;

    constructor(apiKey: string) {
        this.apiKey = apiKey;
    }

    identify(userId: string, traits?: Record<string, any>) {
        this.userId = userId;
        // Send to analytics service
        console.log('Identify:', { userId, traits });
    }

    track(event: string, properties?: Record<string, any>) {
        const payload = {
            event,
            userId: this.userId,
            properties: {
                ...properties,
                timestamp: new Date().toISOString(),
                url: typeof window !== 'undefined' ? window.location.href : '',
                referrer: typeof document !== 'undefined' ? document.referrer : '',
                userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : ''
            }
        };

        // Send to analytics endpoint
        if (typeof fetch !== 'undefined') {
            fetch('/api/analytics/track', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            }).catch(err => console.error('Analytics error:', err));
        }

        // Also log to console in development
        if (process.env.NODE_ENV === 'development') {
            console.log('Track:', payload);
        }
    }

    page(pageName: string, properties?: Record<string, any>) {
        this.track('page_viewed', {
            page: pageName,
            ...properties
        });
    }
}

// Initialize analytics
export const analytics = new Analytics(process.env.ANALYTICS_API_KEY!);
