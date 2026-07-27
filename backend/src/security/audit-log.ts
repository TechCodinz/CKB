export enum AuditAction {
    USER_LOGIN = 'user.login',
    USER_LOGOUT = 'user.logout',
    USER_CREATED = 'user.created',
    USER_UPDATED = 'user.updated',
    USER_DELETED = 'user.deleted',
    PROJECT_CREATED = 'project.created',
    PROJECT_SCANNED = 'project.scanned',
    API_KEY_CREATED = 'api_key.created',
    API_KEY_REVOKED = 'api_key.revoked',
    API_KEY_USED = 'api_key.used',
    API_KEY_UPDATED = 'api_key.updated',
}

export interface AuditLogEntry {
    tenantId: string;
    userId?: string;
    action: AuditAction;
    resourceType?: string;
    resourceId?: string;
    details: Record<string, any>;
    ipAddress?: string;
    userAgent?: string;
    timestamp: Date;
}

export class AuditService {
    async log(entry: AuditLogEntry) {
        console.log('🔒 AUDIT LOG:', JSON.stringify(entry));

        if (process.env.SIEM_WEBHOOK) {
            await fetch(process.env.SIEM_WEBHOOK, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(entry),
            }).catch(err => console.error('SIEM Error:', err));
        }
    }
}

export const auditService = new AuditService();
