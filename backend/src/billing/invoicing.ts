import { PrismaClient } from '@prisma/client';
import { PDFDocument, rgb, StandardFonts } from 'pdf-lib';
import Stripe from 'stripe';

const prisma = new PrismaClient();
const stripe = new Stripe(process.env.STRIPE_SECRET_KEY as string);

export class InvoicingService {
    // Generate invoice PDF
    async generateInvoice(invoiceId: string): Promise<Buffer> {
        const invoice = await prisma.invoice.findUnique({
            where: { id: invoiceId },
            include: {
                customer: true,
                items: true,
            },
        });

        if (!invoice) throw new Error('Invoice not found');

        // Create PDF
        const pdfDoc = await PDFDocument.create();
        const page = pdfDoc.addPage([612, 792]); // US Letter

        const { width, height } = page.getSize();
        const font = await pdfDoc.embedFont(StandardFonts.Helvetica);
        const boldFont = await pdfDoc.embedFont(StandardFonts.HelveticaBold);

        // Header
        page.drawText('INVOICE', {
            x: 50,
            y: height - 50,
            size: 24,
            font: boldFont,
            color: rgb(0.1, 0.1, 0.1),
        });

        // Invoice details
        page.drawText(`Invoice #: ${invoice.number}`, {
            x: 50,
            y: height - 80,
            size: 10,
            font,
        });

        page.drawText(`Date: ${invoice.createdAt.toLocaleDateString()}`, {
            x: 50,
            y: height - 95,
            size: 10,
            font,
        });

        page.drawText(`Due Date: ${invoice.dueDate.toLocaleDateString()}`, {
            x: 50,
            y: height - 110,
            size: 10,
            font,
        });

        // Customer info
        page.drawText('Bill To:', {
            x: 400,
            y: height - 80,
            size: 10,
            font: boldFont,
        });

        page.drawText(invoice.customer?.name || "Unknown Customer", {
            x: 400,
            y: height - 95,
            size: 10,
            font,
        });

        page.drawText(invoice.customer?.email || "", {
            x: 400,
            y: height - 110,
            size: 10,
            font,
        });

        if (invoice.customer?.address) {
            page.drawText((invoice.customer as any).address, {
                x: 400,
                y: height - 125,
                size: 10,
                font,
            });
        }

        // Items table header
        let y = height - 150;
        page.drawLine({
            start: { x: 50, y },
            end: { x: width - 50, y },
            thickness: 1,
            color: rgb(0.8, 0.8, 0.8),
        });

        y -= 20;
        page.drawText('Description', { x: 50, y, size: 10, font: boldFont });
        page.drawText('Qty', { x: 400, y, size: 10, font: boldFont });
        page.drawText('Price', { x: 450, y, size: 10, font: boldFont });
        page.drawText('Amount', { x: 520, y, size: 10, font: boldFont });

        y -= 10;
        page.drawLine({
            start: { x: 50, y },
            end: { x: width - 50, y },
            thickness: 1,
            color: rgb(0.8, 0.8, 0.8),
        });

        // Items
        for (const item of invoice.items) {
            y -= 20;

            // Wrap long descriptions
            const description = item.description;
            if (description.length > 40) {
                page.drawText(description.substring(0, 40), { x: 50, y, size: 9, font });
                y -= 12;
                page.drawText(description.substring(40, 80), { x: 50, y, size: 9, font });
            } else {
                page.drawText(description, { x: 50, y, size: 9, font });
            }

            page.drawText(item.quantity.toString(), { x: 400, y, size: 9, font });
            page.drawText(`$${item.unitPrice.toFixed(2)}`, { x: 450, y, size: 9, font });
            page.drawText(`$${item.amount.toFixed(2)}`, { x: 520, y, size: 9, font });
        }

        // Totals
        y -= 30;
        page.drawLine({
            start: { x: 400, y },
            end: { x: width - 50, y },
            thickness: 1,
            color: rgb(0.8, 0.8, 0.8),
        });

        y -= 20;
        page.drawText('Subtotal:', { x: 450, y, size: 10, font });
        page.drawText(`$${invoice.subtotal.toFixed(2)}`, { x: 520, y, size: 10, font });

        if (invoice.tax > 0) {
            y -= 20;
            page.drawText('Tax:', { x: 450, y, size: 10, font });
            page.drawText(`$${invoice.tax.toFixed(2)}`, { x: 520, y, size: 10, font });
        }

        y -= 20;
        page.drawText('Total:', { x: 450, y, size: 12, font: boldFont });
        page.drawText(`$${invoice.total.toFixed(2)}`, { x: 520, y, size: 12, font: boldFont });

        // Payment instructions
        y -= 50;
        page.drawText('Payment Instructions:', {
            x: 50,
            y,
            size: 10,
            font: boldFont,
        });

        y -= 20;
        page.drawText('Please pay via wire transfer to:', {
            x: 50,
            y,
            size: 9,
            font,
        });

        y -= 15;
        page.drawText('Bank: Silicon Valley Bank', {
            x: 50,
            y,
            size: 9,
            font,
        });

        y -= 15;
        page.drawText('Account: 1234567890', {
            x: 50,
            y,
            size: 9,
            font,
        });

        y -= 15;
        page.drawText('Routing: 121140399', {
            x: 50,
            y,
            size: 9,
            font,
        });

        y -= 15;
        page.drawText('Reference: ' + invoice.number, {
            x: 50,
            y,
            size: 9,
            font,
        });

        // Footer
        page.drawText('Thank you for your business!', {
            x: 50,
            y: 50,
            size: 9,
            font,
            color: rgb(0.4, 0.4, 0.4),
        });

        const pdfBytes = await pdfDoc.save();
        return Buffer.from(pdfBytes.buffer as ArrayBuffer);
    }

    // Create invoice from Stripe data
    async createFromStripe(stripeInvoice: Stripe.Invoice, tenantId: string) {
        const invoice = await prisma.invoice.create({
            data: {
                number: stripeInvoice.number!,
                tenantId,
                stripeInvoiceId: stripeInvoice.id,
                subtotal: stripeInvoice.subtotal / 100,
                tax: (stripeInvoice.tax || 0) / 100,
                total: stripeInvoice.total / 100,
                currency: stripeInvoice.currency,
                status: stripeInvoice.status!,
                dueDate: new Date(stripeInvoice.due_date! * 1000),
                createdAt: new Date(stripeInvoice.created * 1000),
                items: {
                    create: stripeInvoice.lines.data.map(line => ({
                        description: line.description!,
                        quantity: line.quantity || 1,
                        unitPrice: line.unit_amount! / 100,
                        amount: line.amount / 100,
                    })),
                },
            },
        });

        return invoice;
    }

    // Send invoice email
    async sendInvoice(invoiceId: string, email: string) {
        const invoice = await prisma.invoice.findUnique({
            where: { id: invoiceId },
        });

        if (!invoice) throw new Error('Invoice not found');

        const pdf = await this.generateInvoice(invoiceId);

        // Send via email service
        await fetch('https://api.sendgrid.com/v3/mail/send', {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${process.env.SENDGRID_API_KEY}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                personalizations: [{ to: [{ email }] }],
                from: { email: 'billing@ckb.dev', name: 'CKB Billing' },
                subject: `Invoice ${invoice.number} from CKB`,
                content: [{ type: 'text/plain', value: 'Your invoice is attached.' }],
                attachments: [{
                    content: pdf.toString('base64'),
                    filename: `invoice-${invoice.number}.pdf`,
                    type: 'application/pdf',
                }],
            }),
        });
    }

    // Generate quote for enterprise
    async generateQuote(data: {
        tenantId: string;
        customerName: string;
        customerEmail: string;
        items: Array<{ description: string; quantity: number; unitPrice: number }>;
        validUntil: Date;
    }): Promise<Buffer> {
        // Similar to invoice generation but with "QUOTE" header
        // Implementation similar to generateInvoice with different template
        return Buffer.from(''); // Placeholder
    }
}

export const invoicingService = new InvoicingService();
