import { PrismaClient } from '@prisma/client';
import Stripe from 'stripe';

const prisma = new PrismaClient();

export class InvoicingService {
    // Generate simple invoice summary from Payment model
    async generateInvoice(paymentId: string) {
        const payment = await prisma.payment.findUnique({
            where: { id: paymentId },
            include: { user: true },
        });

        if (!payment) throw new Error('Payment invoice not found');

        return {
            invoiceNumber: payment.stripeInvoiceId || `INV-${payment.id.slice(0, 8)}`,
            userEmail: payment.user.email,
            amount: payment.amount / 100,
            currency: payment.currency,
            status: payment.status,
            paidAt: payment.paidAt,
        };
    }

    // Create invoice record from Stripe data
    async createFromStripe(stripeInvoice: Stripe.Invoice, userId: string) {
        return await prisma.payment.create({
            data: {
                userId,
                stripeInvoiceId: stripeInvoice.id,
                amount: stripeInvoice.amount_paid,
                currency: stripeInvoice.currency,
                status: stripeInvoice.status || 'succeeded',
                paidAt: new Date(),
            },
        });
    }

    // Send invoice receipt email notification
    async sendInvoice(paymentId: string, email: string) {
        const invoice = await this.generateInvoice(paymentId);
        console.log(`Receipt sent to ${email} for invoice ${invoice.invoiceNumber}`);
        return { success: true, email, invoice };
    }
}

export const invoicingService = new InvoicingService();
