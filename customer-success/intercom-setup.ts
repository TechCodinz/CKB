// Intercom integration for live chat and customer success

export const intercomConfig = {
    appId: process.env.INTERCOM_APP_ID,
    apiKey: process.env.INTERCOM_API_KEY,
};

export class IntercomService {
    // Identify user in Intercom
    async identifyUser(user: {
        id: string;
        email: string;
        name: string;
        plan: string;
        createdAt: Date;
    }) {
        await fetch('https://api.intercom.io/contacts', {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${process.env.INTERCOM_ACCESS_TOKEN}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                role: 'user',
                external_id: user.id,
                email: user.email,
                name: user.name,
                signed_up_at: user.createdAt.getTime() / 1000,
                custom_attributes: {
                    plan: user.plan,
                },
            }),
        });
    }

    // Track event
    async trackEvent(userId: string, eventName: string, metadata: any) {
        await fetch('https://api.intercom.io/events', {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${process.env.INTERCOM_ACCESS_TOKEN}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                event_name: eventName,
                created_at: Math.floor(Date.now() / 1000),
                user_id: userId,
                metadata,
            }),
        });
    }

    // Send in-app message
    async sendMessage(userId: string, message: string) {
        await fetch('https://api.intercom.io/messages', {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${process.env.INTERCOM_ACCESS_TOKEN}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                message_type: 'inapp',
                body: message,
                from: {
                    type: 'admin',
                    id: process.env.INTERCOM_ADMIN_ID,
                },
                to: {
                    type: 'user',
                    id: userId,
                },
            }),
        });
    }

    // Create help center article
    async createArticle(title: string, content: string, parentId?: string) {
        await fetch('https://api.intercom.io/articles', {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${process.env.INTERCOM_ACCESS_TOKEN}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                title,
                description: content.substring(0, 200),
                body: `<html><body>${content}</body></html>`,
                parent_id: parentId,
                state: 'published',
            }),
        });
    }
}

export const intercomService = new IntercomService();
