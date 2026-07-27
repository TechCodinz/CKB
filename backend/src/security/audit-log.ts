import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

export enum AuditAction {
    // Authentication
    USER_LOGIN = 'user.login',
    USER_LOGOUT = 'user.logout',
    USER_CREATED = 'user.created',
    USER_UPDATED = 'user.updated',
    USER_DELETED = 'user.deleted',
    PASSWORD_CHANGED = 'password.changed',
    MFA_ENABLED = 'mfa.enabled',
    MFA_DISABLED = 'mfa.disabled',

    // Authorization
    PERMISSION_GRANTED = 'permission.granted',
    PERMISSION_REVOKED = 'permission.revoked',
    ROLE_ASSIGNED = 'role.assigned',
    ROLE_REMOVED = 'role.removed',

    // Projects
    PROJECT_CREATED = 'project.created',
    PROJECT_UPDATED = 'project.updated',
    PROJECT_DELETED = 'project.deleted',
    PROJECT_ARCHIVED = 'project.archived',
    PROJECT_SCANNED = 'project.scanned',

    // API
    API_KEY_CREATED = 'api_key.created',
    API_KEY_REVOKED = 'api_key.revoked',
    API_KEY_USED = 'api_key.used',

    // Security
    SETTINGS_CHANGED = 'settings.changed',
    INTEGRATION_ADDED = 'integration.added',
    INTEGRATION_REMOVED = 'integration.removed',
    WEBHOOK_CREATED = 'webhook.created',
    WEBHOOK_UPDATED = 'webhook.updated',
    WEBHOOK_DELETED = 'webhook.deleted',

    // Billing
    SUBSCRIPTION_CHANGED = 'subscription.changed',
    PAYMENT_METHOD_ADDED = 'payment_method.added',
    PAYMENT_METHOD_REMOVED = 'payment_method.removed',
    INVOICE_GENERATED = 'invoice.generated',
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
        // Store in database
        await prisma.auditLog.create({
            data: {
                tenantId: entry.tenantId,
                userId: entry.userId,
                action: entry.action as string,
                resourceType: entry.resourceType,
                resourceId: entry.resourceId,
                details: entry.details,
                ipAddress: entry.ipAddress,
                userAgent: entry.userAgent,
                timestamp: entry.timestamp,
            },
        });

        // Also log to security monitoring
        console.log('AUDIT:', JSON.stringify(entry));

        // Send to SIEM if configured
        if (process.env.SIEM_WEBHOOK) {
            await fetch(process.env.SIEM_WEBHOOK, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(entry),
            }).catch(err => console.error('SIEM error:', err));
        }
    }

    async query(
        tenantId: string,
        filters: {
            userId?: string;
            action?: AuditAction[];
            resourceType?: string;
            resourceId?: string;
            startDate?: Date;
            endDate?: Date;
            limit?: number;
            offset?: number;
        }
    ) {
        const where: any = { tenantId };

        if (filters.userId) where.userId = filters.userId;
        if (filters.action?.length) where.action = { in: filters.action.map(a => a as string) };
        if (filters.resourceType) where.resourceType = filters.resourceType;
        if (filters.resourceId) where.resourceId = filters.resourceId;
        if (filters.startDate || filters.endDate) {
            where.timestamp = {};
            if (filters.startDate) where.timestamp.gte = filters.startDate;
            if (filters.endDate) where.timestamp.lte = filters.endDate;
        }

        return await prisma.auditLog.findMany({
            where,
            orderBy: { timestamp: 'desc' },
            take: filters.limit || 100,
            skip: filters.offset || 0,
        });
    }

    async export(tenantId: string, format: 'json' | 'csv' | 'syslog', filters: any) {
        const logs = await this.query(tenantId, { ...filters, limit: 10000 });

        switch (format) {
            case 'json':
                return JSON.stringify(logs, null, 2);

            case 'csv':
                const headers = ['timestamp', 'userId', 'action', 'resourceType', 'resourceId', 'details', 'ipAddress'];
                const rows = logs.map(log => [
                    log.timestamp.toISOString(),
                    log.userId || '',
                    log.action,
                    log.resourceType || '',
                    log.resourceId || '',
                    JSON.stringify(log.details),
                    log.ipAddress || '',
                ]);
                return [headers.join(','), ...rows.map(r => r.join(','))].join('\n');

            case 'syslog':
                return logs.map(log =>
                    `<${log.userId ? 'info' : 'warn'}> ${log.timestamp.toISOString()} ckb[${log.tenantId}]: ${log.action} ${JSON.stringify(log.details)}`
                ).join('\n');
        }
    }

    async getStats(tenantId: string, days: number = 30) {
        const startDate = new Date();
        startDate.setDate(startDate.getDate() - days);

        const logs = await prisma.auditLog.groupBy({
            by: ['action'],
            where: {
                tenantId,
                timestamp: { gte: startDate },
            },
            _count: true,
        });

        return logs;
    }
}

export const auditService = new AuditService();
