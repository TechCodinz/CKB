import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

const FLW_SECRET_KEY = process.env.FLUTTERWAVE_SECRET_KEY || 'FLWSECK_TEST-sandbox-secret-key';
const FLW_SECRET_HASH = process.env.FLUTTERWAVE_SECRET_HASH || 'ckb-flw-secret-hash';
const FLW_API_URL = 'https://api.flutterwave.com/v3';

export interface FlutterwaveInitializeParams {
    userId: string;
    email: string;
    name: string;
    planId: string;
    amount: number;
    currency?: string;
    redirectUrl: string;
}

export class FlutterwavePaymentService {
    /**
     * Initialize a Flutterwave checkout session for subscription payment
     */
    async initializePayment(params: FlutterwaveInitializeParams) {
        const txRef = `ckb_flw_${Date.now()}_${Math.floor(Math.random() * 1000)}`;

        const payload = {
            tx_ref: txRef,
            amount: params.amount,
            currency: params.currency || 'USD',
            redirect_url: params.redirectUrl,
            meta: {
                userId: params.userId,
                planId: params.planId,
            },
            customer: {
                email: params.email,
                name: params.name,
            },
            customizations: {
                title: 'CKB - Architectural Intelligence',
                description: `Subscription for ${params.planId.toUpperCase()} Plan`,
                logo: 'https://ckb.dev/assets/logo.png',
            },
        };

        try {
            const res = await fetch(`${FLW_API_URL}/payments`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${FLW_SECRET_KEY}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(payload),
            });

            const data: any = await res.json();

            if (data && data.status === 'success') {
                return {
                    status: 'success',
                    paymentUrl: data.data.link,
                    txRef,
                };
            } else {
                throw new Error(data.message || 'Failed to initialize Flutterwave payment');
            }
        } catch (error: any) {
            console.error('Flutterwave Initialization Error:', error.message);
            throw new Error(error.message || 'Flutterwave payment initialization failed');
        }
    }

    /**
     * Verify Flutterwave transaction by ID
     */
    async verifyTransaction(transactionId: string) {
        try {
            const res = await fetch(`${FLW_API_URL}/transactions/${transactionId}/verify`, {
                method: 'GET',
                headers: {
                    Authorization: `Bearer ${FLW_SECRET_KEY}`,
                },
            });

            const data: any = await res.json();

            if (data && data.status === 'success') {
                const txData = data.data;
                
                if (txData.status === 'successful') {
                    const userId = txData.meta?.userId;
                    const planId = txData.meta?.planId || 'pro';

                    if (userId) {
                        await this.provisionSubscription(userId, planId, txData.id.toString(), txData.amount);
                    }

                    return {
                        verified: true,
                        amount: txData.amount,
                        currency: txData.currency,
                        customer: txData.customer,
                    };
                }
            }

            return { verified: false, reason: 'Transaction unverified or incomplete' };
        } catch (error: any) {
            console.error('Flutterwave Verification Error:', error.message);
            throw new Error('Flutterwave transaction verification failed');
        }
    }

    /**
     * Handle incoming Webhook events from Flutterwave
     */
    async handleWebhook(body: any, signature: string) {
        // Verify secret hash signature
        if (signature !== FLW_SECRET_HASH) {
            throw new Error('Invalid Flutterwave webhook signature');
        }

        if (body.event === 'charge.completed' && body.data.status === 'successful') {
            const txData = body.data;
            const userId = txData.meta?.userId;
            const planId = txData.meta?.planId || 'pro';

            if (userId) {
                await this.provisionSubscription(userId, planId, txData.id.toString(), txData.amount);
            }
        }

        return { status: 'acknowledged' };
    }

    private async provisionSubscription(userId: string, planId: string, transactionId: string, amount: number) {
        await prisma.user.update({
            where: { id: userId },
            data: {
                subscription: {
                    create: {
                        stripeSubscriptionId: `flw_${transactionId}`,
                        planId,
                        status: 'active',
                        currentPeriodStart: new Date(),
                        currentPeriodEnd: new Date(Date.now() + 30 * 24 * 60 * 60 * 1000),
                    },
                },
            },
        });

        console.log(`✅ Flutterwave Subscription Provisioned for User ${userId} (Plan: ${planId}, Tx: ${transactionId})`);
    }
}

export const flutterwavePaymentService = new FlutterwavePaymentService();
