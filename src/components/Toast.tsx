type ToastProps = {
    message: string;
};

export const Toast = ({ message }: ToastProps) => {
    return (
        <div className="toast" role="status" aria-live="polite">
            {message}
        </div>
    );
};
